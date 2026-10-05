//! Live observations coordinate independent system adapters.
use super::{
    alf::{read_applications, read_settings},
    cache::Controls,
};
use crate::{activity::Monitor, geoip::GeoIp};
use anyhow::{Result, ensure};
use rooklet_core::model::{NetworkStatus, Snapshot};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct Live {
    monitor: Option<Monitor>,
    activity_error: Option<String>,
    geoip: GeoIp,
    geoip_error: Option<String>,
    controls: Controls,
    resources: crate::resources::Sampler,
    interest: rooklet_core::resources::ResourceInterest,
}
impl Live {
    pub(super) fn new() -> Result<Self> {
        ensure!(cfg!(target_os = "macos"), "firewall controls require macOS");
        let (mut geoip, mut geoip_error) = match GeoIp::managed() {
            Ok(geoip) => (geoip, None),
            Err(error) => (
                GeoIp::default(),
                Some(format!("Country database unavailable: {error:#}")),
            ),
        };
        if geoip_error.is_none() {
            geoip_error = geoip
                .refresh()
                .err()
                .map(|error| format!("Country database unavailable: {error:#}"));
        }
        let (monitor, activity_error) = match Monitor::start() {
            Ok(monitor) => (Some(monitor), None),
            Err(error) => (None, Some(format!("Activity unavailable: {error}"))),
        };
        Ok(Self {
            monitor,
            activity_error,
            geoip,
            geoip_error,
            controls: Controls::default(),
            resources: crate::resources::Sampler::default(),
            interest: rooklet_core::resources::ResourceInterest::default(),
        })
    }
    pub(super) fn reload_geoip(&mut self) -> Result<()> {
        if !self.geoip.is_managed() {
            self.geoip = GeoIp::managed()?;
        }
        self.geoip.refresh()?;
        self.geoip_error = None;
        Ok(())
    }
    pub(super) fn set_resource_interest(
        &mut self,
        interest: &rooklet_core::resources::ResourceInterest,
    ) {
        self.interest = interest.clone();
    }
    pub(super) fn snapshot(
        &mut self,
        cancel: &AtomicBool,
        force_controls: bool,
    ) -> Result<Snapshot> {
        // A separate CLI update should become visible in an already-running TUI.
        if self.geoip.is_managed() {
            self.geoip_error = self
                .geoip
                .refresh()
                .err()
                .map(|error| format!("Country database refresh failed: {error:#}"));
        }
        let mut snapshot = self.controls.read(force_controls, || read_controls(cancel));
        snapshot.geoip = self.geoip.description();
        if let Some(monitor) = &mut self.monitor {
            match monitor.poll(&mut self.geoip, cancel) {
                Ok(activity) => {
                    snapshot.activity = activity;
                    if !monitor.has_sample() {
                        snapshot.notices.push("Activity is warming up: nettop has not yet produced a complete sample.".into());
                    }
                }
                Err(error) => snapshot
                    .notices
                    .push(format!("Activity unavailable: {error}")),
            }
        }
        if let Some(error) = &self.activity_error {
            snapshot.notices.push(error.clone());
        }
        snapshot.resources = self.resources.observe(&self.interest, cancel);
        let identities = self.resources.identities();
        attach_identities(&mut snapshot, &identities);
        if snapshot.geoip.is_none() {
            snapshot.notices.push("Countries are Unknown until you run rooklet geoip update or press g in Settings. Local and link-local peers are identified offline.".into());
        }
        if let Some(error) = &self.geoip_error {
            snapshot.notices.push(error.clone());
        }
        snapshot.notices.push("Traffic totals start at the first observation. Activity is best effort; hidden processes and wildcard UDP peers may be unavailable. Application firewall controls incoming connections.".into());
        snapshot.permission_paths = crate::permissions::capture_paths(&snapshot);
        ensure!(!cancel.load(Ordering::Relaxed), "snapshot cancelled");
        Ok(snapshot)
    }
}
fn read_network_status(cancel: &AtomicBool) -> NetworkStatus {
    let result = crate::network::request_status(cancel);
    result.unwrap_or_else(|error| NetworkStatus {
        message: Some(format!(
            "Network status unavailable: {error}. Administrator access may be required."
        )),
        ..NetworkStatus::default()
    })
}

fn read_controls(cancel: &AtomicBool) -> Snapshot {
    let mut snapshot = Snapshot {
        network: read_network_status(cancel),
        ..Snapshot::default()
    };
    match read_settings(cancel) {
        Ok(settings) => snapshot.firewall = Some(settings),
        Err(error) => snapshot.notices.push(format!(
            "Application firewall settings unavailable: {error}"
        )),
    }
    match read_applications(cancel) {
        Ok(apps) => {
            snapshot.applications = apps;
            snapshot.applications_available = true;
        }
        Err(error) => snapshot
            .notices
            .push(format!("Application firewall list unavailable: {error}")),
    }
    snapshot
}

// Build membership once; each app includes verified helpers with no network traffic.
fn attach_identities(
    snapshot: &mut Snapshot,
    identities: &[rooklet_core::process::ProcessIdentity],
) {
    use std::collections::BTreeMap;
    let by_pid: BTreeMap<_, _> = identities
        .iter()
        .map(|identity| (identity.pid, identity))
        .collect();
    let mut bundles: BTreeMap<(&str, u32), Vec<rooklet_core::process::ProcessIdentity>> =
        BTreeMap::new();
    for identity in identities {
        if let Some(bundle) = identity.bundle_path.as_deref() {
            bundles
                .entry((bundle, identity.uid))
                .or_default()
                .push(identity.clone());
        }
    }
    for activity in &mut snapshot.activity {
        let Some(identity) = by_pid.get(&activity.pid) else {
            continue;
        };
        if !activity
            .path
            .as_ref()
            .is_some_and(|path| identity.bundle_path.as_ref().unwrap_or(&identity.path) == path)
        {
            continue;
        }
        activity.identities = match identity.bundle_path.as_deref() {
            Some(bundle) => bundles
                .get(&(bundle, identity.uid))
                .cloned()
                .unwrap_or_default(),
            None => vec![(*identity).clone()],
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rooklet_core::model::ProcessActivity;
    use rooklet_core::process::ProcessIdentity;

    fn identity(pid: u32, uid: u32, bundle: &str) -> ProcessIdentity {
        ProcessIdentity {
            pid,
            uid,
            parent_pid: 1,
            start_sec: 1,
            start_usec: 0,
            pid_version: 1,
            path: format!("{bundle}/Contents/MacOS/Process{pid}"),
            bundle_path: Some(bundle.into()),
        }
    }

    #[test]
    fn observed_apps_include_non_network_helpers_only_for_the_same_owner_and_bundle() {
        let main = identity(10, 1000, "/Applications/Browser.app");
        let helper = identity(11, 1000, "/Applications/Browser.app");
        let other_owner = identity(12, 1001, "/Applications/Browser.app");
        let other_bundle = identity(13, 1000, "/Applications/Other.app");
        let mut snapshot = Snapshot {
            activity: vec![ProcessActivity {
                pid: 10,
                name: "Browser".into(),
                path: main.bundle_path.clone(),
                identities: Vec::new(),
                bytes_in: 0,
                bytes_out: 0,
                rate_in: 0,
                rate_out: 0,
                connections: Vec::new(),
            }],
            ..Default::default()
        };
        attach_identities(
            &mut snapshot,
            &[main.clone(), helper.clone(), other_owner, other_bundle],
        );
        assert_eq!(snapshot.activity[0].identities, [main, helper]);
        snapshot.activity[0].identities.clear();
        snapshot.activity[0].path = Some("/Applications/Replaced.app".into());
        attach_identities(
            &mut snapshot,
            &[identity(10, 1000, "/Applications/Browser.app")],
        );
        assert!(snapshot.activity[0].identities.is_empty());
    }
}
