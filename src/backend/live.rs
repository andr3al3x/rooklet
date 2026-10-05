//! Live observations coordinate independent system adapters.
use super::alf::{read_applications, read_settings};
use crate::{
    activity::Monitor,
    command,
    geoip::GeoIp,
    model::{NetworkStatus, Snapshot},
};
use anyhow::{Context, Result, ensure};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct Live {
    monitor: Option<Monitor>,
    activity_error: Option<String>,
    geoip: GeoIp,
    geoip_error: Option<String>,
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
    pub(super) fn snapshot(&mut self, cancel: &AtomicBool) -> Result<Snapshot> {
        // A separate CLI update should become visible in an already-running TUI.
        if self.geoip.is_managed() {
            self.geoip_error = self
                .geoip
                .refresh()
                .err()
                .map(|error| format!("Country database refresh failed: {error:#}"));
        }
        let mut snapshot = Snapshot {
            geoip: self.geoip.description(),
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
        match crate::process::capture_snapshot() {
            Ok(identities) => {
                for activity in &mut snapshot.activity {
                    if let Some(identity) = identities
                        .iter()
                        .find(|identity| identity.pid == activity.pid)
                    {
                        let matches = activity.path.as_ref().is_some_and(|path| {
                            identity.bundle_path.as_ref().unwrap_or(&identity.path) == path
                        });
                        if matches {
                            activity.identities =
                                crate::process::members_for(&identities, activity.pid);
                        }
                    }
                }
            }
            Err(error) => snapshot
                .notices
                .push(format!("App termination unavailable: {error:#}")),
        }
        if snapshot.geoip.is_none() {
            snapshot.notices.push("Countries are Unknown until you run xield geoip update or press g in Settings. Local and link-local peers are identified offline.".into());
        }
        if let Some(error) = &self.geoip_error {
            snapshot.notices.push(error.clone());
        }
        snapshot.notices.push("Traffic totals start at the first observation. Activity is best effort; hidden processes and wildcard UDP peers may be unavailable. Application firewall controls incoming connections.".into());
        ensure!(!cancel.load(Ordering::Relaxed), "snapshot cancelled");
        Ok(snapshot)
    }
}
fn read_network_status(cancel: &AtomicBool) -> NetworkStatus {
    let result = (|| -> Result<NetworkStatus> {
        let executable = std::env::current_exe()?.canonicalize()?;
        let output = command::run(
            &executable,
            &["network".into(), "status".into()],
            None,
            true,
            cancel,
        )?;
        serde_json::from_str(&output).context("invalid network status response")
    })();
    result.unwrap_or_else(|error| NetworkStatus {
        message: Some(format!(
            "Network status unavailable: {error}. Administrator access may be required."
        )),
        ..NetworkStatus::default()
    })
}
