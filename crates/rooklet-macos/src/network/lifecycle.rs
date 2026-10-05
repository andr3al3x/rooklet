//! Serialized setup, apply, disable, and removal transactions with rollback.
use super::{
    ANCHOR_FILE, CONFIG,
    config::{
        configured, has_parent_anchor, normalized_rules, parent_matches_managed_config,
        patch_config, recovery_config, setup_needs_reload,
    },
    persistence::{
        STATE_FILE, State, atomic_write, backup_config, bounded_read, mutation_lock, prepare_state,
        read_state, save_state, trusted, trusted_parents,
    },
    pf::{
        acquire_token, check_parent_rules, load_anchor, root, run, token_is_live, validate_pf,
        verify_interfaces,
    },
};
use anyhow::{Context, Result, ensure};
use rooklet_core::model::{NetworkRule, NetworkStatus};
use rooklet_core::network::compile_rules;
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

fn persistent_rules_match(state: &State, source: &str) -> Result<bool> {
    Ok(source == compile_rules(if state.active { &state.rules } else { &[] })?)
}
pub(super) fn status() -> NetworkStatus {
    let mut status = NetworkStatus::default();
    let result = (|| -> Result<()> {
        let source = bounded_read(Path::new(CONFIG))?;
        status.configured = configured(&source);
        let state = read_state()?;
        status.rules = state.rules.clone();
        status.rules_available = true;
        let persisted_matches = if status.configured {
            trusted_parents(Path::new(ANCHOR_FILE))?;
            trusted(Path::new(ANCHOR_FILE), false)?;
            persistent_rules_match(&state, &bounded_read(Path::new(ANCHOR_FILE))?)?
        } else {
            false
        };
        let info = run(&["-s", "info"], None)
            .context("Cannot inspect PF (noninteractive sudo access is required)")?;
        status.enabled = info
            .lines()
            .any(|line| line.trim().starts_with("Status: Enabled"));
        let parent = run(&["-sr"], None)?;
        let actual = normalized_rules(&run(&["-a", "rooklet", "-sr"], None)?);
        status.applied = status.configured
            && status.enabled
            && parent_matches_managed_config(&parent)
            && persisted_matches
            && state.active
            && state.loaded_rules.as_deref() == Some(actual.as_str());
        status.message = Some(if status.applied {
            "PF is enabled, the parent references Rooklet, and its loaded rules match the saved snapshot. Existing states and other PF anchors may affect traffic; this is not proof of enforcement. Rules are not automatically reapplied after reboot.".into()
        } else if state.active {
            "Saved Rooklet rules are not confirmed active: PF, the parent rules, the persistent anchor file, or the loaded rules differ. Apply again after checking PF. Existing states are retained.".into()
        } else {
            "Rooklet rules are inactive. Other PF users may keep PF enabled. Existing states are retained.".into()
        });
        Ok(())
    })();
    if let Err(error) = result {
        status.message = Some(format!("{error:#}"));
    }
    status
}

pub(super) fn setup(rules: &[NetworkRule]) -> Result<()> {
    root()?;
    let _lock = mutation_lock()?;
    setup_inner(rules)
}
fn setup_inner(rules: &[NetworkRule]) -> Result<()> {
    let _span = tracing::debug_span!("pf_lifecycle", phase = "setup").entered();
    let compiled = compile_rules(rules)?;
    verify_interfaces(rules)?;
    trusted_parents(Path::new(CONFIG))?;
    trusted(Path::new(CONFIG), false)?;
    trusted_parents(Path::new(ANCHOR_FILE))?;
    let config_mode = fs::metadata(CONFIG)?.permissions().mode() & 0o777;
    let original = bounded_read(Path::new(CONFIG))?;
    let updated = patch_config(&original, true)?;
    let parent_referenced = has_parent_anchor(&run(&["-sr"], None)?);
    let mut prior_state = read_state()?;
    let token_live = if let Some(token) = &prior_state.enable_token {
        token_is_live(token)?
    } else {
        false
    };
    let live_anchor = normalized_rules(&run(&["-a", "rooklet", "-sr"], None)?);
    if !setup_needs_reload(
        configured(&original),
        parent_referenced,
        &live_anchor,
        token_live,
    )? {
        return apply_inner(rules);
    }
    check_parent_rules()?;
    trusted_parents(Path::new("/etc/pf.anchors/com.apple"))?;
    trusted(Path::new("/etc/pf.anchors/com.apple"), false)?;
    let previous_anchor = if Path::new(ANCHOR_FILE).exists() {
        trusted(Path::new(ANCHOR_FILE), false)?;
        let previous = bounded_read(Path::new(ANCHOR_FILE))?;
        ensure!(
            previous.starts_with("# Rooklet network rules:"),
            "An unmanaged /etc/pf.anchors/rooklet exists; refusing to overwrite it"
        );
        Some(previous)
    } else {
        None
    };
    validate_pf(&compiled)?;
    // Validate recovery without a dependency on the managed anchor file.
    let recovery_source = recovery_config(&original)?;
    run(&["-n", "-f", "-"], Some(recovery_source.as_bytes()))?;
    prepare_state()?;
    // Explicit setup can recover after reboot: only a token proven absent from
    // PF references is discarded, and orphaned live rules are never adopted.
    prior_state.enable_token = None;
    prior_state.active = false;
    prior_state.loaded_rules = None;
    save_state(&prior_state)?;
    let backup = backup_config(&original)?;
    // Install an empty persistent anchor first: enabling/loading the actual rules is
    // a separate transaction, so failed setup cannot leave untracked rules active.
    if let Err(error) = atomic_write(
        Path::new(ANCHOR_FILE),
        compile_rules(&[])?.as_bytes(),
        0o600,
    ) {
        let restore = restore_anchor(previous_anchor.as_deref());
        return Err(error.context(format!(
            "Could not install persistent Rooklet anchor; no live rules were changed. Anchor file restoration: {}",
            restoration_result(restore)
        )));
    }
    if let Err(error) = run(&["-n", "-f", "-"], Some(updated.as_bytes())) {
        let restore = restore_anchor(previous_anchor.as_deref());
        return Err(error.context(format!(
            "Root PF configuration validation failed; no live rules were changed. Anchor file restoration: {}",
            restoration_result(restore)
        )));
    }
    if let Err(error) = atomic_write(Path::new(CONFIG), updated.as_bytes(), config_mode) {
        let config_restore = atomic_write(Path::new(CONFIG), original.as_bytes(), config_mode);
        let anchor_restore = restore_anchor(previous_anchor.as_deref());
        return Err(error.context(format!(
            "Could not install parent PF configuration; no live rules were changed. Parent file restoration: {}; anchor file restoration: {}",
            restoration_result(config_restore),
            restoration_result(anchor_restore)
        )));
    }
    reload_with_recovery(
        &recovery_source,
        &format!("PF setup reload failed (backup: {})", backup.display()),
        true,
        &mut ParentFiles {
            original: &original,
            config_mode,
            previous_anchor: previous_anchor.as_deref(),
        },
    )?;
    apply_inner(rules).with_context(|| format!("Parent anchor installed (backup: {}), but applying rules failed; run network disable or retry apply", backup.display()))
}
fn restore_anchor(previous: Option<&str>) -> Result<()> {
    let result = (|| {
        if let Some(previous) = previous {
            atomic_write(Path::new(ANCHOR_FILE), previous.as_bytes(), 0o600)
        } else {
            if let Err(error) = fs::symlink_metadata(ANCHOR_FILE) {
                return if error.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(error.into())
                };
            }
            trusted(Path::new(ANCHOR_FILE), false)?;
            fs::remove_file(ANCHOR_FILE).map_err(Into::into)
        }
    })();
    if result.is_err() {
        tracing::error!(phase = "restore", "PF persistent anchor restoration failed");
    } else {
        tracing::debug!(phase = "restore", "PF persistent anchor restored");
    }
    result
}

// A deliberately small seam for failure injection: these are exactly the four
// independent recovery actions after a partially successful parent reload.
trait ParentRecovery {
    fn reload(&mut self, source: Option<&str>) -> Result<()>;
    fn restore_config(&mut self) -> Result<()>;
    fn restore_anchor(&mut self) -> Result<()>;
    fn restore_runtime_anchor(&mut self) -> Result<()>;
}

struct ParentFiles<'a> {
    original: &'a str,
    config_mode: u32,
    previous_anchor: Option<&'a str>,
}
impl ParentRecovery for ParentFiles<'_> {
    fn reload(&mut self, source: Option<&str>) -> Result<()> {
        match source {
            Some(source) => run(&["-f", "-"], Some(source.as_bytes())),
            None => run(&["-f", CONFIG], None),
        }
        .map(|_| ())
    }
    fn restore_config(&mut self) -> Result<()> {
        atomic_write(
            Path::new(CONFIG),
            self.original.as_bytes(),
            self.config_mode,
        )
    }
    fn restore_anchor(&mut self) -> Result<()> {
        restore_anchor(self.previous_anchor)
    }
    fn restore_runtime_anchor(&mut self) -> Result<()> {
        // Setup reloads only with an empty prior live anchor. Removal keeps it
        // disabled. Neither recovery can read the uncertain persistent file.
        load_anchor("")
    }
}

fn restoration_result(result: Result<()>) -> String {
    match result {
        Ok(()) => "succeeded".into(),
        Err(error) => format!("failed ({error:#})"),
    }
}

fn reload_with_recovery(
    original: &str,
    context: &str,
    restore_anchor: bool,
    recovery: &mut impl ParentRecovery,
) -> Result<()> {
    let Err(error) = recovery.reload(None) else {
        return Ok(());
    };
    tracing::error!(
        phase = "reload",
        "PF parent reload failed; restoring prior configuration"
    );
    let config = recovery.restore_config();
    let anchor = restore_anchor.then(|| recovery.restore_anchor());
    // The validated recovery source excludes Rooklet's load-anchor directive,
    // so neither failed file restoration can affect this parent reload.
    let runtime = recovery.reload(Some(original));
    let runtime_anchor = recovery.restore_runtime_anchor();
    if config.is_err()
        || anchor.as_ref().is_some_and(Result::is_err)
        || runtime.is_err()
        || runtime_anchor.is_err()
    {
        tracing::error!(
            phase = "restore",
            file_restored = config.is_ok(),
            anchor_restored = anchor.as_ref().is_none_or(Result::is_ok),
            live_restored = runtime.is_ok(),
            live_anchor_restored = runtime_anchor.is_ok(),
            "PF parent restoration failed"
        );
    } else {
        tracing::info!(outcome = "restored", "PF parent configuration restored");
    }
    let mut outcome = format!(
        "{context}. Parent file restoration: {}",
        restoration_result(config)
    );
    if let Some(anchor) = anchor {
        outcome.push_str(&format!(
            "; anchor file restoration: {}",
            restoration_result(anchor)
        ));
    }
    outcome.push_str(&format!(
        "; runtime parent restoration: {}; runtime anchor restoration: {}",
        restoration_result(runtime),
        restoration_result(runtime_anchor)
    ));
    Err(error.context(outcome))
}

pub(super) fn apply(rules: &[NetworkRule]) -> Result<()> {
    root()?;
    let _lock = mutation_lock()?;
    apply_inner(rules)
}
fn apply_inner(rules: &[NetworkRule]) -> Result<()> {
    let _span = tracing::debug_span!("pf_lifecycle", phase = "apply").entered();
    let super::preflight::Validated {
        source: compiled,
        expected,
        previous,
    } = super::preflight::validate(rules)?;
    prepare_state()?;
    let mut prior = read_state()?;
    if let Some(token) = &prior.enable_token
        && !token_is_live(token)?
    {
        prior.enable_token = None;
        save_state(&prior)?;
    }
    let previous_live = normalized_rules(&run(&["-a", "rooklet", "-sr"], None)?);
    // Record the new ownership token before loading any rules, so a later failure can release it.
    let mut next = prior.clone();
    if next.enable_token.is_none() {
        next.enable_token = Some(acquire_token()?);
        if let Err(error) = save_state(&next) {
            let release = run(&["-X", next.enable_token.as_deref().unwrap()], None);
            if release.is_err() {
                tracing::error!(
                    phase = "restore",
                    "PF ownership release failed after persistence failure"
                );
            }
            return Err(error.context(format!(
                "Could not persist PF ownership token {}; token release: {:?}",
                next.enable_token.as_deref().unwrap(),
                release.err()
            )));
        }
    }
    let operation = (|| -> Result<()> {
        atomic_write(Path::new(ANCHOR_FILE), compiled.as_bytes(), 0o600)?;
        load_anchor(&compiled)?;
        next.rules = rules.to_vec();
        next.active = true;
        let actual = normalized_rules(&run(&["-a", "rooklet", "-sr"], None)?);
        ensure!(
            actual == expected,
            "PF loaded rules differ from the validated preview; refusing to record them as applied"
        );
        tracing::info!(
            outcome = "verified",
            rule_count = rules.len(),
            "PF loaded anchor readback verified"
        );
        next.loaded_rules = Some(expected);
        save_state(&next)
    })();
    if let Err(error) = operation {
        return recover_apply(
            error,
            prior.enable_token.is_none(),
            &mut ApplyFiles {
                previous: &previous,
                previous_live: &previous_live,
                prior: &prior,
                next: &next,
            },
        );
    }
    Ok(())
}

// Exactly the recovery actions for an ordinary anchor apply, independently of
// parent setup/removal. The seam permits failure tests without touching live PF.
trait ApplyRecovery {
    fn restore_file(&mut self) -> Result<()>;
    fn restore_live(&mut self) -> Result<()>;
    fn release_reference(&mut self) -> Result<()>;
    fn restore_state(&mut self) -> Result<()>;
}

struct ApplyFiles<'a> {
    previous: &'a str,
    previous_live: &'a str,
    prior: &'a State,
    next: &'a State,
}
impl ApplyRecovery for ApplyFiles<'_> {
    fn restore_file(&mut self) -> Result<()> {
        atomic_write(Path::new(ANCHOR_FILE), self.previous.as_bytes(), 0o600)
    }
    fn restore_live(&mut self) -> Result<()> {
        load_anchor(self.previous_live)
    }
    fn release_reference(&mut self) -> Result<()> {
        run(&["-X", self.next.enable_token.as_deref().unwrap()], None).map(|_| ())
    }
    fn restore_state(&mut self) -> Result<()> {
        save_state(self.prior)
    }
}

fn recover_apply(
    error: anyhow::Error,
    new_reference: bool,
    recovery: &mut impl ApplyRecovery,
) -> Result<()> {
    tracing::error!(
        phase = "apply",
        "PF anchor application or readback failed; restoring prior anchor"
    );
    let _span = tracing::debug_span!("pf_restore").entered();
    let restore_file = recovery.restore_file();
    let restore_live = recovery.restore_live();
    if restore_file.is_err() || restore_live.is_err() {
        tracing::error!(
            file_restored = restore_file.is_ok(),
            live_restored = restore_live.is_ok(),
            "PF anchor restoration failed"
        );
        return Err(error.context(format!("Anchor rollback failed (file: {:?}; live: {:?}); ownership token remains recorded; run network disable", restore_file.err(), restore_live.err())));
    }
    if new_reference && let Err(release) = recovery.release_reference() {
        tracing::error!(
            phase = "restore",
            "PF ownership release failed during restoration"
        );
        return Err(error.context(format!(
            "Apply failed and PF ownership release failed: {release:#}; token remains recorded"
        )));
    }
    if let Err(restore) = recovery.restore_state() {
        tracing::error!(phase = "restore", "PF saved state restoration failed");
        return Err(error.context(format!(
            "Apply failed and PF saved state restoration failed: {restore:#}"
        )));
    }
    tracing::info!(outcome = "restored", "previous PF anchor restored");
    Err(error.context("Applying Rooklet rules failed; previous anchor restored"))
}

pub(super) fn disable() -> Result<()> {
    root()?;
    let _lock = mutation_lock()?;
    disable_inner()
}
fn disable_inner() -> Result<()> {
    let _span = tracing::debug_span!("pf_lifecycle", phase = "disable").entered();
    prepare_state()?;
    let mut state = read_state()?;
    // An empty anchor clears only Rooklet rules. Never flush states or disable global PF.
    load_anchor("")?;
    // A later manual root reload must not reactivate disabled rules.
    if Path::new(ANCHOR_FILE).exists() {
        trusted(Path::new(ANCHOR_FILE), false)?;
        ensure!(
            bounded_read(Path::new(ANCHOR_FILE))?.starts_with("# Rooklet network rules:"),
            "Rooklet's anchor file was replaced; live anchor cleared but persistent file retained"
        );
        atomic_write(
            Path::new(ANCHOR_FILE),
            compile_rules(&[])?.as_bytes(),
            0o600,
        )?;
    }
    state.active = false;
    state.loaded_rules = None;
    save_state(&state)?;
    if let Some(token) = state.enable_token.clone() {
        if token_is_live(&token)? {
            run(&["-X", &token], None).context("Rooklet anchor cleared, but releasing its PF reference failed; ownership token retained for retry")?;
        }
        state.enable_token = None;
        save_state(&state)?;
    }
    // Empty the persistent anchor too; a later manual root reload must not reactivate disabled rules.
    if Path::new(ANCHOR_FILE).exists() {
        atomic_write(
            Path::new(ANCHOR_FILE),
            compile_rules(&[])?.as_bytes(),
            0o600,
        )?;
    }
    Ok(())
}

pub(super) fn remove() -> Result<()> {
    root()?;
    let _lock = mutation_lock()?;
    remove_inner()
}
fn remove_inner() -> Result<()> {
    let _span = tracing::debug_span!("pf_lifecycle", phase = "remove").entered();
    trusted_parents(Path::new(CONFIG))?;
    trusted(Path::new(CONFIG), false)?;
    let config_mode = fs::metadata(CONFIG)?.permissions().mode() & 0o777;
    let original = bounded_read(Path::new(CONFIG))?;
    let updated = patch_config(&original, false)?;
    ensure!(
        configured(&original),
        "Rooklet's managed parent anchor is absent; no root configuration changes made"
    );
    check_parent_rules()?;
    trusted_parents(Path::new("/etc/pf.anchors/com.apple"))?;
    trusted(Path::new("/etc/pf.anchors/com.apple"), false)?;
    let recovery_source = recovery_config(&original)?;
    run(&["-n", "-f", "-"], Some(recovery_source.as_bytes()))?;
    run(&["-n", "-f", "-"], Some(updated.as_bytes()))?;
    let backup = backup_config(&original)?;
    disable_inner()?;
    if let Err(error) = atomic_write(Path::new(CONFIG), updated.as_bytes(), config_mode) {
        let restore = atomic_write(Path::new(CONFIG), original.as_bytes(), config_mode);
        return Err(error.context(format!(
            "Could not install parent PF configuration during removal; Rooklet remains disabled. Parent file restoration: {}",
            restoration_result(restore)
        )));
    }
    reload_with_recovery(
        &recovery_source,
        &format!(
            "Removal reload failed (backup: {}). Rooklet remains disabled",
            backup.display()
        ),
        false,
        &mut ParentFiles {
            original: &original,
            config_mode,
            previous_anchor: None,
        },
    )?;
    trusted(Path::new(ANCHOR_FILE), false)?;
    fs::remove_file(ANCHOR_FILE)?;
    trusted(Path::new(STATE_FILE), false)?;
    fs::remove_file(STATE_FILE)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ApplyStep {
        File,
        Live,
        Reference,
        State,
    }

    struct FakeApply {
        fail: Option<ApplyStep>,
        calls: Vec<ApplyStep>,
        prior: State,
        recorded: State,
        reference_live: bool,
    }
    impl FakeApply {
        fn step(&mut self, step: ApplyStep) -> Result<()> {
            self.calls.push(step);
            ensure!(self.fail != Some(step), "{step:?} recovery failed");
            Ok(())
        }
    }
    impl ApplyRecovery for FakeApply {
        fn restore_file(&mut self) -> Result<()> {
            self.step(ApplyStep::File)
        }
        fn restore_live(&mut self) -> Result<()> {
            self.step(ApplyStep::Live)
        }
        fn release_reference(&mut self) -> Result<()> {
            self.step(ApplyStep::Reference)?;
            self.reference_live = false;
            Ok(())
        }
        fn restore_state(&mut self) -> Result<()> {
            self.step(ApplyStep::State)?;
            self.recorded = self.prior.clone();
            Ok(())
        }
    }

    #[test]
    fn apply_recovery_preserves_primary_error_and_owned_reference_until_safe_release() {
        use ApplyStep::{File, Live, Reference, State as SavedState};
        for (failure, new_reference, expected_calls) in [
            (Some(File), true, vec![File, Live]),
            (Some(Live), true, vec![File, Live]),
            (Some(Reference), true, vec![File, Live, Reference]),
            (
                Some(SavedState),
                true,
                vec![File, Live, Reference, SavedState],
            ),
            (None, true, vec![File, Live, Reference, SavedState]),
            (None, false, vec![File, Live, SavedState]),
        ] {
            let prior = State {
                enable_token: (!new_reference).then(|| "owned-reference".into()),
                ..Default::default()
            };
            let recorded = State {
                enable_token: Some("owned-reference".into()),
                ..prior.clone()
            };
            let mut fake = FakeApply {
                fail: failure,
                calls: vec![],
                prior,
                recorded,
                reference_live: true,
            };
            let error = recover_apply(
                anyhow::anyhow!("loaded rules differ from preview"),
                new_reference,
                &mut fake,
            )
            .unwrap_err();
            assert_eq!(
                error.root_cause().to_string(),
                "loaded rules differ from preview"
            );
            if let Some(step) = failure {
                assert!(format!("{error:#}").contains(&format!("{step:?} recovery failed")));
            }
            assert_eq!(fake.calls, expected_calls);
            assert_eq!(
                fake.reference_live,
                !new_reference || matches!(failure, Some(File | Live | Reference))
            );
            assert_eq!(
                fake.recorded.enable_token.is_some(),
                !new_reference || failure.is_some()
            );
        }
    }

    #[derive(Default)]
    struct FakeParent {
        reload_failure: bool,
        config_failure: bool,
        anchor_failure: bool,
        runtime_failure: bool,
        runtime_anchor_failure: bool,
        calls: Vec<String>,
    }
    impl ParentRecovery for FakeParent {
        fn reload(&mut self, source: Option<&str>) -> Result<()> {
            if let Some(source) = source {
                self.calls.push(format!("runtime:{source}"));
                ensure!(!self.runtime_failure, "runtime restoration failed");
            } else {
                self.calls.push("reload".into());
                ensure!(!self.reload_failure, "original reload failed");
            }
            Ok(())
        }
        fn restore_config(&mut self) -> Result<()> {
            self.calls.push("config".into());
            ensure!(!self.config_failure, "config restoration failed");
            Ok(())
        }
        fn restore_anchor(&mut self) -> Result<()> {
            self.calls.push("anchor".into());
            ensure!(!self.anchor_failure, "persistent anchor restoration failed");
            Ok(())
        }
        fn restore_runtime_anchor(&mut self) -> Result<()> {
            self.calls.push("runtime-anchor".into());
            ensure!(
                !self.runtime_anchor_failure,
                "runtime anchor restoration failed"
            );
            Ok(())
        }
    }

    #[test]
    fn setup_reload_attempts_every_recovery_and_preserves_all_failures() {
        for failures in 0..16 {
            let mut fake = FakeParent {
                reload_failure: true,
                config_failure: failures & 1 != 0,
                anchor_failure: failures & 2 != 0,
                runtime_failure: failures & 4 != 0,
                runtime_anchor_failure: failures & 8 != 0,
                ..Default::default()
            };
            let error =
                reload_with_recovery("validated original", "setup", true, &mut fake).unwrap_err();
            assert_eq!(
                fake.calls,
                [
                    "reload",
                    "config",
                    "anchor",
                    "runtime:validated original",
                    "runtime-anchor"
                ]
            );
            assert_eq!(error.root_cause().to_string(), "original reload failed");
            let report = format!("{error:#}");
            for (failed, message) in [
                (fake.config_failure, "config restoration failed"),
                (fake.anchor_failure, "persistent anchor restoration failed"),
                (fake.runtime_failure, "runtime restoration failed"),
                (
                    fake.runtime_anchor_failure,
                    "runtime anchor restoration failed",
                ),
            ] {
                assert_eq!(report.contains(message), failed, "{report}");
            }
        }
    }

    #[test]
    fn removal_reload_recovers_parent_independently_without_reactivating_anchor() {
        for failures in 0..8 {
            let mut fake = FakeParent {
                reload_failure: true,
                config_failure: failures & 1 != 0,
                runtime_failure: failures & 2 != 0,
                runtime_anchor_failure: failures & 4 != 0,
                ..Default::default()
            };
            let error =
                reload_with_recovery("validated original", "remove; disabled", false, &mut fake)
                    .unwrap_err();
            assert_eq!(
                fake.calls,
                [
                    "reload",
                    "config",
                    "runtime:validated original",
                    "runtime-anchor"
                ]
            );
            let report = format!("{error:#}");
            assert!(report.contains("original reload failed"));
            assert_eq!(
                report.contains("config restoration failed"),
                fake.config_failure
            );
            assert_eq!(
                report.contains("runtime restoration failed"),
                fake.runtime_failure
            );
            assert_eq!(
                report.contains("runtime anchor restoration failed"),
                fake.runtime_anchor_failure
            );
        }
    }

    #[test]
    fn successful_parent_reload_does_not_run_recovery() {
        let mut fake = FakeParent::default();
        reload_with_recovery("validated original", "setup", true, &mut fake).unwrap();
        assert_eq!(fake.calls, ["reload"]);
    }

    #[test]
    fn persistent_file_drift_is_detected() {
        let state = State::default();
        let source = compile_rules(&[]).unwrap();
        assert!(persistent_rules_match(&state, &source).unwrap());
        assert!(!persistent_rules_match(&state, "pass all\n").unwrap());
    }
}
