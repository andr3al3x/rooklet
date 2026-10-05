use crate::application::{registration_path, validate_existing_path};
use anyhow::{Context, Result, ensure};
use rooklet_core::model::{Application, FirewallSettings, NetworkRule, Profile, Snapshot};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io::Read,
    path::Path,
};

use rooklet_core::profile::{MAX_BYTES, export, validate};
#[derive(Debug, Clone)]
pub struct Prepared {
    pub(super) profile: Profile,
    pub(super) baseline: Baseline,
}
impl Prepared {
    pub fn review(&self) -> String {
        super::review::render(self)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Baseline {
    pub(super) firewall: FirewallSettings,
    pub(super) applications: Vec<Application>,
    pub(super) rules: Vec<NetworkRule>,
    pub(super) configured: bool,
    pub(super) enabled: bool,
    pub(super) applied: bool,
}
impl Baseline {
    pub(super) fn capture(snapshot: &Snapshot) -> Result<Self> {
        let profile = export(snapshot)?;
        Ok(Self {
            firewall: profile.firewall.context("firewall unavailable")?,
            applications: profile.applications,
            rules: profile.network_rules,
            configured: snapshot.network.configured,
            enabled: snapshot.network.enabled,
            applied: snapshot.network.applied,
        })
    }
    pub(super) fn same_configuration(&self, other: &Self) -> bool {
        self.firewall == other.firewall
            && permissions(&self.applications) == permissions(&other.applications)
            && self.rules == other.rules
            && self.configured == other.configured
            && self.enabled == other.enabled
            && self.applied == other.applied
    }
    pub(super) fn profile(&self) -> Profile {
        Profile {
            format: "rooklet-profile".into(),
            version: 1,
            firewall: Some(self.firewall.clone()),
            applications: self.applications.clone(),
            network_rules: self.rules.clone(),
        }
    }
}
pub(super) fn permissions(apps: &[Application]) -> Vec<(&str, bool)> {
    let mut entries: Vec<_> = apps
        .iter()
        .map(|app| (app.path.as_str(), app.blocked))
        .collect();
    entries.sort_unstable();
    entries
}
pub(super) fn read(path: &Path) -> Result<Profile> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options
        .open(path)
        .with_context(|| format!("cannot open profile {}", path.display()))?;
    read_file(file)
}
pub(super) fn read_file(file: File) -> Result<Profile> {
    let metadata = file.metadata()?;
    ensure!(metadata.is_file(), "profile must be a regular file");
    ensure!(
        metadata.len() <= MAX_BYTES as u64,
        "configuration exceeds 1 MiB"
    );
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_BYTES, "configuration exceeds 1 MiB");
    rooklet_core::profile::parse(&bytes)
}
pub(super) fn prepare(profile: &Profile, snapshot: &Snapshot) -> Result<Prepared> {
    prepare_with(profile, snapshot, &mut registration_path, &mut |path| {
        validate_existing_path(path)
    })
}
fn prepare_with(
    profile: &Profile,
    snapshot: &Snapshot,
    resolve: &mut impl FnMut(&str) -> Result<String>,
    check_path: &mut impl FnMut(&str) -> Result<()>,
) -> Result<Prepared> {
    validate(profile)?;
    let baseline = Baseline::capture(snapshot)?;
    ensure!(
        profile.network_rules.is_empty() || baseline.configured,
        "set up PF before applying a profile containing network rules"
    );
    let profile = normalize_applications(profile, &baseline.applications, resolve)?;
    for app in &profile.applications {
        check_path(&app.path)?;
    }
    validate(&profile)?;
    Ok(Prepared { profile, baseline })
}
fn normalize_applications(
    profile: &Profile,
    current: &[Application],
    resolve: &mut impl FnMut(&str) -> Result<String>,
) -> Result<Profile> {
    let mut profile = profile.clone();
    let mut paths = HashSet::new();
    for app in &mut profile.applications {
        rooklet_core::application::validate_path(&app.path)?;
        if !current.iter().any(|old| old.path == app.path) {
            app.path = resolve(&app.path)?;
        }
        ensure!(
            paths.insert(app.path.clone()),
            "duplicate application registration after resolving paths: {:?}",
            app.path
        );
    }
    Ok(profile)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn entry(path: &str) -> Application {
        Application {
            path: path.into(),
            name: "App".into(),
            blocked: false,
        }
    }
    fn snapshot() -> Snapshot {
        Snapshot {
            firewall: Some(Default::default()),
            applications_available: true,
            network: rooklet_core::model::NetworkStatus {
                rules_available: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }
    #[test]
    fn exact_alias_is_retained_and_new_bundle_resolves() {
        let mut current = snapshot();
        current.applications = vec![entry("/registered/Alias.app")];
        let mut profile = Profile::from_snapshot(&current);
        profile.applications.push(entry("/Applications/New.app"));
        let prepared = prepare_with(
            &profile,
            &current,
            &mut |path| {
                assert_eq!(path, "/Applications/New.app");
                Ok("/canonical/NewExecutable".into())
            },
            &mut |_| Ok(()),
        )
        .unwrap();
        assert_eq!(
            prepared.profile.applications[0].path,
            "/registered/Alias.app"
        );
        assert_eq!(
            prepared.profile.applications[1].path,
            "/canonical/NewExecutable"
        );
    }
    #[test]
    fn normalization_rejects_duplicate_aliases() {
        let current = snapshot();
        let mut profile = Profile::from_snapshot(&current);
        profile.applications = vec![entry("/A.app"), entry("/B.app")];
        assert!(
            prepare_with(
                &profile,
                &current,
                &mut |_| Ok("/canonical/App".into()),
                &mut |_| Ok(())
            )
            .is_err()
        );
    }
    #[test]
    fn export_rejects_each_unavailable_scope() {
        let mut current = snapshot();
        current.firewall = None;
        assert!(export(&current).is_err());
        current = snapshot();
        current.applications_available = false;
        assert!(export(&current).is_err());
        current = snapshot();
        current.network.rules_available = false;
        assert!(export(&current).is_err());
    }
    #[test]
    fn pure_validation_accepts_missing_files_but_preparation_checks_all_targets() {
        let current = snapshot();
        let mut profile = Profile::from_snapshot(&current);
        profile.applications = vec![entry("/nonexistent/App")];
        validate(&profile).unwrap();
        let mut checked = false;
        assert!(
            prepare_with(&profile, &current, &mut |path| Ok(path.into()), &mut |_| {
                checked = true;
                anyhow::bail!("missing")
            })
            .is_err()
        );
        assert!(checked);
    }
}
