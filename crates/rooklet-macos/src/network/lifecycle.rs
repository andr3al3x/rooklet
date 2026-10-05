//! Serialized setup, apply, disable, and removal transactions with rollback.
use super::{
    ANCHOR_FILE, CONFIG,
    config::{
        configured, has_parent_anchor, normalized_rules, parent_matches_managed_config,
        patch_config, setup_needs_reload,
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
    validate_pf(&compiled)?;
    prepare_state()?;
    // Explicit setup can recover after reboot: only a token proven absent from
    // PF references is discarded, and orphaned live rules are never adopted.
    prior_state.enable_token = None;
    prior_state.active = false;
    prior_state.loaded_rules = None;
    save_state(&prior_state)?;
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
    let backup = backup_config(&original)?;
    // Install an empty persistent anchor first: enabling/loading the actual rules is
    // a separate transaction, so failed setup cannot leave untracked rules active.
    atomic_write(
        Path::new(ANCHOR_FILE),
        compile_rules(&[])?.as_bytes(),
        0o600,
    )?;
    if let Err(error) = run(&["-n", "-f", "-"], Some(updated.as_bytes())) {
        restore_anchor(previous_anchor.as_deref())?;
        return Err(
            error.context("Root PF configuration validation failed; no live rules were changed")
        );
    }
    if let Err(error) = atomic_write(Path::new(CONFIG), updated.as_bytes(), config_mode) {
        restore_anchor(previous_anchor.as_deref())?;
        return Err(
            error.context("Could not install parent PF configuration; anchor file restored")
        );
    }
    if let Err(error) = run(&["-f", CONFIG], None) {
        atomic_write(Path::new(CONFIG), original.as_bytes(), config_mode)?;
        restore_anchor(previous_anchor.as_deref())?;
        // A failed PF reload may have partially changed live rules; restoring the validated original is necessary.
        let rollback = run(&["-f", CONFIG], None);
        return Err(error.context(format!(
            "PF setup failed; restored config from {}. Runtime restore: {:?}",
            backup.display(),
            rollback.err()
        )));
    }
    apply_inner(rules).with_context(|| format!("Parent anchor installed (backup: {}), but applying rules failed; run network disable or retry apply", backup.display()))
}
fn restore_anchor(previous: Option<&str>) -> Result<()> {
    if let Some(previous) = previous {
        atomic_write(Path::new(ANCHOR_FILE), previous.as_bytes(), 0o600)
    } else {
        trusted(Path::new(ANCHOR_FILE), false)?;
        fs::remove_file(ANCHOR_FILE).map_err(Into::into)
    }
}

pub(super) fn apply(rules: &[NetworkRule]) -> Result<()> {
    root()?;
    let _lock = mutation_lock()?;
    apply_inner(rules)
}
fn apply_inner(rules: &[NetworkRule]) -> Result<()> {
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
        next.loaded_rules = Some(expected);
        save_state(&next)
    })();
    if let Err(error) = operation {
        let restore_file = atomic_write(Path::new(ANCHOR_FILE), previous.as_bytes(), 0o600);
        let restore_live = load_anchor(&previous_live);
        if restore_file.is_err() || restore_live.is_err() {
            return Err(error.context(format!("Anchor rollback failed (file: {:?}; live: {:?}); ownership token remains recorded; run network disable", restore_file.err(), restore_live.err())));
        }
        if prior.enable_token.is_none() {
            run(&["-X", next.enable_token.as_deref().unwrap()], None)
                .context("Apply failed and PF ownership release failed; token remains recorded")?;
        }
        save_state(&prior)?;
        return Err(error.context("Applying Rooklet rules failed; previous anchor restored"));
    }
    Ok(())
}

pub(super) fn disable() -> Result<()> {
    root()?;
    let _lock = mutation_lock()?;
    disable_inner()
}
fn disable_inner() -> Result<()> {
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
    run(&["-n", "-f", "-"], Some(updated.as_bytes()))?;
    let backup = backup_config(&original)?;
    disable_inner()?;
    atomic_write(Path::new(CONFIG), updated.as_bytes(), config_mode)?;
    if let Err(error) = run(&["-f", CONFIG], None) {
        atomic_write(Path::new(CONFIG), original.as_bytes(), config_mode)?;
        let rollback = run(&["-f", CONFIG], None);
        return Err(error.context(format!("Removal reload failed; parent config restored (backup: {}). Rooklet remains disabled. Runtime restore: {:?}", backup.display(), rollback.err())));
    }
    trusted(Path::new(ANCHOR_FILE), false)?;
    fs::remove_file(ANCHOR_FILE)?;
    trusted(Path::new(STATE_FILE), false)?;
    fs::remove_file(STATE_FILE)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistent_file_and_parent_order_drift_are_detected() {
        let state = State::default();
        let source = compile_rules(&[]).unwrap();
        assert!(persistent_rules_match(&state, &source).unwrap());
        assert!(!persistent_rules_match(&state, "pass all\n").unwrap());
        let parent = "anchor \"com.apple/*\" all\nanchor \"rooklet\" all\n";
        assert!(parent_matches_managed_config(parent));
        assert!(!parent_matches_managed_config(&format!(
            "pass quick all\n{parent}"
        )));
        assert!(!parent_matches_managed_config(
            "anchor \"rooklet\" all\nanchor \"com.apple/*\" all\n"
        ));
        assert!(!parent_matches_managed_config("anchor \"rooklet\" all"));
    }
}
