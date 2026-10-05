//! Best-effort multi-scope application with configuration drift and readback checks.
use super::preparation::{Baseline, Prepared, permissions};
use crate::{
    backend::Backend,
    model::{Action, Application, Mutation, NetworkRule, Profile, Setting, Snapshot},
};
use anyhow::{Context, Result, bail, ensure};

trait Operations {
    fn snapshot(&mut self) -> Result<Snapshot>;
    fn mutate(&mut self, mutation: Mutation) -> Result<()>;
    fn preflight_network(&mut self, rules: &[NetworkRule]) -> Result<()>;
}
impl Operations for Backend {
    fn snapshot(&mut self) -> Result<Snapshot> {
        self.snapshot()
    }
    fn mutate(&mut self, mutation: Mutation) -> Result<()> {
        self.mutate(mutation)
    }
    fn preflight_network(&mut self, rules: &[NetworkRule]) -> Result<()> {
        self.preflight_network_rules(rules)
    }
}
pub(super) fn apply(backend: &mut Backend, prepared: &Prepared) -> Result<()> {
    apply_with(backend, prepared)
}
fn apply_with(backend: &mut impl Operations, prepared: &Prepared) -> Result<()> {
    let before = Baseline::capture(&backend.snapshot()?)?;
    ensure!(
        prepared.baseline.same_configuration(&before),
        "configuration changed since profile review; review the profile again"
    );
    // Recheck targets before the first mutation, including after authentication.
    for app in &prepared.profile.applications {
        backend_path_check(&app.path)?;
    }
    if before.configured {
        backend
            .preflight_network(&prepared.profile.network_rules)
            .context("network profile preflight failed; no profile scopes were changed")?;
        let after_preflight = Baseline::capture(&backend.snapshot()?)?;
        ensure!(
            prepared.baseline.same_configuration(&after_preflight),
            "configuration changed during profile preflight; review the profile again"
        );
    }
    transact(backend, prepared)
}
fn backend_path_check(path: &str) -> Result<()> {
    crate::backend::validate_application_path(path, true)
}
fn transact(backend: &mut impl Operations, prepared: &Prepared) -> Result<()> {
    let before = &prepared.baseline;
    let mut network_attempted = false;
    let operation = (|| -> Result<()> {
        apply_permissions_with(&before.applications, &prepared.profile, &mut |mutation| {
            backend.mutate(mutation)
        })?;
        if before.configured {
            network_attempted = true;
            backend.mutate(Mutation::NetworkRules(
                prepared.profile.network_rules.clone(),
            ))?;
        }
        verify_profile(&backend.snapshot()?, &prepared.profile, before.configured)?;
        Ok(())
    })();
    if let Err(error) = operation {
        let incoming = restore_permissions(backend, &before.profile());
        let network = if network_attempted {
            restore_network(backend, before)
        } else {
            Ok("untouched".into())
        };
        bail!(
            "profile apply failed: {error}; restoration: incoming={}, network={}",
            report(incoming),
            report(network)
        );
    }
    Ok(())
}
fn report(result: Result<String>) -> String {
    result.unwrap_or_else(|error| format!("FAILED ({error})"))
}
fn restore_permissions(backend: &mut impl Operations, old: &Profile) -> Result<String> {
    let current = backend
        .snapshot()
        .context("cannot inspect incoming entries for restoration")?;
    ensure!(
        current.applications_available,
        "incoming entries unavailable during restoration"
    );
    apply_permissions_with(&current.applications, old, &mut |mutation| {
        backend.mutate(mutation)
    })?;
    let restored = backend.snapshot()?;
    ensure!(
        restored.firewall == old.firewall
            && restored.applications_available
            && permissions(&restored.applications) == permissions(&old.applications),
        "incoming restoration readback differs from baseline"
    );
    Ok("restored and verified".into())
}
fn restore_network(backend: &mut impl Operations, before: &Baseline) -> Result<String> {
    if let Ok(current) = backend.snapshot()
        && current.network.rules_available
        && current.network.rules == before.rules
        && current.network.configured == before.configured
        && current.network.enabled == before.enabled
        && current.network.applied == before.applied
    {
        return Ok("unchanged and verified".into());
    }
    backend.mutate(Mutation::NetworkRules(before.rules.clone()))?;
    let current = backend.snapshot()?;
    ensure!(
        current.network.rules_available && current.network.rules == before.rules,
        "network rule restoration readback differs from baseline"
    );
    ensure!(
        current.network.configured == before.configured
            && current.network.enabled == before.enabled
            && current.network.applied == before.applied,
        "rule content restored, but prior PF activation/configuration was not restored; no global PF disable was attempted"
    );
    Ok("restored and verified".into())
}
fn verify_profile(snapshot: &Snapshot, profile: &Profile, configured: bool) -> Result<()> {
    ensure!(
        snapshot.firewall == profile.firewall,
        "firewall settings differ from requested profile after apply"
    );
    ensure!(
        snapshot.applications_available
            && permissions(&snapshot.applications) == permissions(&profile.applications),
        "incoming application permissions differ from requested profile after apply"
    );
    ensure!(
        snapshot.network.rules_available
            && snapshot.network.rules == profile.network_rules
            && snapshot.network.configured == configured,
        "saved network configuration differs from requested profile after apply"
    );
    ensure!(
        !configured || (snapshot.network.enabled && snapshot.network.applied),
        "PF is disabled or the Xield anchor is not applied after profile apply"
    );
    Ok(())
}
fn apply_permissions_with(
    old: &[Application],
    profile: &Profile,
    mutate: &mut impl FnMut(Mutation) -> Result<()>,
) -> Result<()> {
    if let Some(settings) = &profile.firewall {
        for setting in [
            Setting::Firewall,
            Setting::Stealth,
            Setting::BlockAll,
            Setting::AllowSigned,
            Setting::AllowSignedApp,
        ] {
            mutate(Mutation::Setting(setting, settings.get(setting)))?;
        }
    }
    for application in &profile.applications {
        if !old.iter().any(|app| app.path == application.path) {
            mutate(Mutation::AddApplication(application.path.clone()))?;
        }
        mutate(Mutation::Applications {
            paths: vec![application.path.clone()],
            action: if application.blocked {
                Action::Block
            } else {
                Action::Allow
            },
        })?;
    }
    for application in old {
        if !profile
            .applications
            .iter()
            .any(|app| app.path == application.path)
        {
            mutate(Mutation::RemoveApplication(application.path.clone()))?;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Fake {
        snapshot: Snapshot,
        mutations: Vec<Mutation>,
        fail_at: Option<usize>,
        fail_readback: bool,
        fail_preflight: bool,
        preflight_drift: bool,
        preflights: Vec<Vec<NetworkRule>>,
        first_network_mismatch: Option<bool>,
        network_mutations: usize,
    }
    impl Operations for Fake {
        fn snapshot(&mut self) -> Result<Snapshot> {
            if self.fail_readback {
                anyhow::bail!("readback unavailable");
            }
            Ok(self.snapshot.clone())
        }
        fn preflight_network(&mut self, rules: &[NetworkRule]) -> Result<()> {
            self.preflights.push(rules.to_vec());
            if self.fail_preflight {
                anyhow::bail!("invalid interface or PF syntax");
            }
            if self.preflight_drift {
                self.snapshot.network.applied = !self.snapshot.network.applied;
            }
            Ok(())
        }
        fn mutate(&mut self, mutation: Mutation) -> Result<()> {
            self.mutations.push(mutation.clone());
            if self.fail_at == Some(self.mutations.len()) {
                anyhow::bail!("injected mutation failure");
            }
            match mutation {
                Mutation::Setting(setting, value) => {
                    self.snapshot.firewall.as_mut().unwrap().set(setting, value)
                }
                Mutation::NetworkRules(rules) => {
                    self.snapshot.network.rules = rules;
                    self.snapshot.network.enabled = true;
                    self.snapshot.network.applied = true;
                    self.network_mutations += 1;
                    if self.network_mutations == 1
                        && let Some(enabled_mismatch) = self.first_network_mismatch
                    {
                        if enabled_mismatch {
                            self.snapshot.network.enabled = false;
                        } else {
                            self.snapshot.network.applied = false;
                        }
                    }
                }
                Mutation::Applications { paths, action } => {
                    for app in &mut self.snapshot.applications {
                        if paths.contains(&app.path) {
                            app.blocked = action == Action::Block;
                        }
                    }
                }
                Mutation::AddApplication(path) => self.snapshot.applications.push(Application {
                    path,
                    name: "App".into(),
                    blocked: false,
                }),
                Mutation::RemoveApplication(path) => {
                    self.snapshot.applications.retain(|app| app.path != path)
                }
            }
            Ok(())
        }
    }
    fn fixture() -> (Fake, Prepared) {
        let snapshot = Snapshot {
            firewall: Some(Default::default()),
            applications_available: true,
            network: crate::model::NetworkStatus {
                rules_available: true,
                configured: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut profile = Profile::from_snapshot(&snapshot);
        profile.firewall.as_mut().unwrap().stealth = true;
        let prepared = Prepared {
            profile,
            baseline: Baseline::capture(&snapshot).unwrap(),
        };
        (
            Fake {
                snapshot,
                mutations: vec![],
                fail_at: None,
                fail_readback: false,
                fail_preflight: false,
                preflight_drift: false,
                preflights: vec![],
                first_network_mismatch: None,
                network_mutations: 0,
            },
            prepared,
        )
    }
    #[test]
    fn changed_review_baseline_rejects_without_mutations() {
        let (mut backend, prepared) = fixture();
        backend.snapshot.network.enabled = true;
        assert!(
            apply_with(&mut backend, &prepared)
                .unwrap_err()
                .to_string()
                .contains("changed since")
        );
        assert!(backend.mutations.is_empty());
    }
    #[test]
    fn success_verifies_all_scopes_and_network_apply_failure_skips_unchanged_pf() {
        let (mut backend, prepared) = fixture();
        apply_with(&mut backend, &prepared).unwrap();
        assert!(backend.snapshot.firewall.unwrap().stealth);
        assert!(backend.snapshot.network.applied);
        let (mut backend, prepared) = fixture();
        backend.fail_at = Some(6);
        let error = transact(&mut backend, &prepared).unwrap_err().to_string();
        assert!(error.contains("network=unchanged and verified"));
        assert_eq!(
            backend
                .mutations
                .iter()
                .filter(|mutation| matches!(mutation, Mutation::NetworkRules(_)))
                .count(),
            1
        );
    }
    #[test]
    fn drift_rejects_firewall_permissions_rules_and_network_flags() {
        let (original, prepared) = fixture();
        for change in 0..5 {
            let mut snapshot = original.snapshot.clone();
            match change {
                0 => snapshot.firewall.as_mut().unwrap().stealth = true,
                1 => snapshot.applications.push(Application {
                    path: "/New".into(),
                    name: "New".into(),
                    blocked: true,
                }),
                2 => snapshot.network.configured = false,
                3 => snapshot.network.applied = true,
                _ => snapshot.network.rules_available = false,
            }
            let mut backend = Fake {
                snapshot,
                mutations: vec![],
                fail_at: None,
                fail_readback: false,
                fail_preflight: false,
                preflight_drift: false,
                preflights: vec![],
                first_network_mismatch: None,
                network_mutations: 0,
            };
            assert!(apply_with(&mut backend, &prepared).is_err());
            assert!(backend.mutations.is_empty());
        }
    }
    #[test]
    fn network_preflight_failure_leaves_all_scopes_and_inactive_pf_untouched() {
        let (mut backend, prepared) = fixture();
        backend.fail_preflight = true;
        let error = apply_with(&mut backend, &prepared).unwrap_err().to_string();
        assert!(error.contains("no profile scopes were changed"));
        assert_eq!(
            backend.preflights,
            vec![prepared.profile.network_rules.clone()]
        );
        assert!(backend.mutations.is_empty());
        assert!(!backend.snapshot.network.enabled);
        assert!(!backend.snapshot.network.applied);
        assert_eq!(
            backend.snapshot.firewall,
            Some(prepared.baseline.firewall.clone())
        );
    }
    #[test]
    fn drift_during_preflight_is_rejected_before_mutation() {
        let (mut backend, prepared) = fixture();
        backend.preflight_drift = true;
        assert!(
            apply_with(&mut backend, &prepared)
                .unwrap_err()
                .to_string()
                .contains("changed during")
        );
        assert!(backend.mutations.is_empty());
    }
    #[test]
    fn configured_network_readback_requires_enabled_and_applied_and_restores_mismatches() {
        for enabled_mismatch in [true, false] {
            let (mut backend, mut prepared) = fixture();
            backend.snapshot.network.enabled = true;
            backend.snapshot.network.applied = true;
            prepared.baseline = Baseline::capture(&backend.snapshot).unwrap();
            prepared.profile.network_rules = vec![NetworkRule {
                id: "new".into(),
                name: "New rule".into(),
                action: Action::Block,
                destination: "any".into(),
                port: None,
                protocol: crate::model::Protocol::Any,
                direction: crate::model::Direction::Both,
                interface: None,
                enabled: true,
            }];
            backend.first_network_mismatch = Some(enabled_mismatch);
            let error = apply_with(&mut backend, &prepared).unwrap_err().to_string();
            assert!(error.contains("PF is disabled or the Xield anchor is not applied"));
            assert!(error.contains("incoming=restored and verified"));
            assert!(error.contains("network=restored and verified"));
            assert_eq!(backend.network_mutations, 2);
            assert!(
                prepared
                    .baseline
                    .same_configuration(&Baseline::capture(&backend.snapshot).unwrap())
            );
        }
    }
    #[test]
    fn incoming_failure_restores_and_never_touches_inactive_pf() {
        let (mut backend, prepared) = fixture();
        backend.fail_at = Some(2);
        let error = transact(&mut backend, &prepared).unwrap_err().to_string();
        assert!(error.contains("incoming=restored and verified"));
        assert!(error.contains("network=untouched"));
        assert!(
            !backend
                .mutations
                .iter()
                .any(|mutation| matches!(mutation, Mutation::NetworkRules(_)))
        );
        assert!(!backend.snapshot.network.enabled);
    }
    #[test]
    fn final_readback_detects_incoming_mismatch() {
        let (mut backend, prepared) = fixture();
        backend.snapshot.firewall.as_mut().unwrap().enabled = true;
        assert!(verify_profile(&backend.snapshot, &prepared.profile, true).is_err());
    }
    #[test]
    fn failed_restoration_is_explicit() {
        let (mut backend, prepared) = fixture();
        backend.fail_readback = true;
        let error = transact(&mut backend, &prepared).unwrap_err().to_string();
        assert!(error.contains("incoming=FAILED"));
    }
    #[test]
    fn network_rollback_never_claims_restored_activation() {
        let (mut backend, prepared) = fixture();
        backend.snapshot.network.enabled = true;
        let error = restore_network(&mut backend, &prepared.baseline)
            .unwrap_err()
            .to_string();
        assert!(error.contains("prior PF activation/configuration was not restored"));
    }
    #[test]
    fn existing_raw_registration_is_not_added_again() {
        let old = vec![Application {
            path: "/registered/Alias.app".into(),
            name: "Alias".into(),
            blocked: false,
        }];
        let profile = Profile {
            format: "xield-profile".into(),
            version: 1,
            firewall: None,
            applications: vec![Application {
                blocked: true,
                ..old[0].clone()
            }],
            network_rules: vec![],
        };
        let mut mutations = vec![];
        apply_permissions_with(&old, &profile, &mut |mutation| {
            mutations.push(mutation);
            Ok(())
        })
        .unwrap();
        assert_eq!(mutations.len(), 1);
        assert!(matches!(
            &mutations[0],
            Mutation::Applications {
                action: Action::Block,
                ..
            }
        ));
    }
}
