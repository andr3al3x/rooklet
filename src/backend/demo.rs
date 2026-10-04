//! Explicit in-memory simulation; no system commands.
use super::application::{application_name, validate_application_path};
use crate::model::{
    Action, Application, Connection, Country, Direction, FirewallSettings, Mutation, NetworkRule,
    NetworkStatus, ProcessActivity, Protocol, Snapshot,
};
use anyhow::{Context, Result, ensure};
use std::time::{Duration, Instant};

pub(super) struct Demo {
    snapshot: Snapshot,
    started: Instant,
}
impl Demo {
    pub(super) fn new() -> Self {
        let applications = vec![
            Application {
                path: "/Applications/Safari.app".into(),
                name: "Safari".into(),
                blocked: false,
            },
            Application {
                path: "/Applications/Updater.app".into(),
                name: "Updater".into(),
                blocked: true,
            },
        ];
        let mut activity: Vec<ProcessActivity> = [
            (
                201,
                "Safari",
                "/Applications/Safari.app",
                "151.101.1.69",
                443,
                Protocol::Tcp,
                Some(Country {
                    code: "US".into(),
                    name: "United States".into(),
                }),
            ),
            (
                202,
                "Terminal",
                "/System/Applications/Utilities/Terminal.app",
                "2606:4700:4700::1111",
                443,
                Protocol::Tcp,
                Some(Country {
                    code: "AU".into(),
                    name: "Australia".into(),
                }),
            ),
            (
                203,
                "mDNSResponder",
                "/usr/sbin/mDNSResponder",
                "224.0.0.251",
                5353,
                Protocol::Udp,
                None,
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (pid, name, path, ip, port, protocol, country))| {
            let rate_in = 8000 * (index as u64 + 1);
            let rate_out = 1600 * (index as u64 + 1);
            ProcessActivity {
                pid,
                name: name.into(),
                path: Some(path.into()),
                identities: vec![crate::process::ProcessIdentity {
                    pid,
                    uid: 1000,
                    parent_pid: 1,
                    start_sec: 1,
                    start_usec: 0,
                    pid_version: 1,
                    path: if path.ends_with(".app") {
                        format!("{path}/Contents/MacOS/{name}")
                    } else {
                        path.into()
                    },
                    bundle_path: path.ends_with(".app").then(|| path.into()),
                }],
                bytes_in: 0,
                bytes_out: 0,
                rate_in,
                rate_out,
                connections: vec![Connection {
                    remote_ip: ip.into(),
                    remote_port: Some(port),
                    protocol,
                    bytes_in: 0,
                    bytes_out: 0,
                    country,
                    local: index == 2,
                }],
            }
        })
        .collect();
        let mut helper = activity[0].identities[0].clone();
        helper.pid = 204;
        helper.parent_pid = 201;
        helper.path = "/Applications/Safari.app/Contents/Frameworks/Safari Helper".into();
        activity[0].identities.push(helper);
        let firewall = FirewallSettings {
            enabled: true,
            stealth: false,
            block_all: false,
            allow_signed: true,
            allow_signed_app: true,
        };
        let rule = NetworkRule {
            id: "demo-updater".into(),
            name: "Example destination block".into(),
            action: Action::Block,
            destination: "203.0.113.42".into(),
            port: Some(443),
            protocol: Protocol::Tcp,
            direction: Direction::Outbound,
            interface: None,
            enabled: true,
        };
        let network = NetworkStatus {
            rules_available: true,
            configured: true,
            enabled: true,
            applied: true,
            rules: vec![rule],
            message: Some("Simulated network rules".into()),
        };
        let notices = vec!["DEMO — simulated traffic and controls; no system changes. ALF settings do not determine simulated outbound activity.".into()];
        let snapshot = Snapshot {
            demo: true,
            applications_available: true,
            firewall: Some(firewall),
            applications,
            activity,
            network,
            geoip: Some("Simulated country data".into()),
            notices,
        };
        Self {
            snapshot,
            started: Instant::now(),
        }
    }
    pub(super) fn snapshot(&mut self) -> Snapshot {
        self.advance(self.started.elapsed());
        self.snapshot.clone()
    }
    fn advance(&mut self, elapsed: Duration) {
        for process in &mut self.snapshot.activity {
            process.bytes_in = ((elapsed.as_millis() * process.rate_in as u128) / 1000)
                .min(u64::MAX as u128) as u64;
            process.bytes_out = ((elapsed.as_millis() * process.rate_out as u128) / 1000)
                .min(u64::MAX as u128) as u64;
            process.connections[0].bytes_in = process.bytes_in;
            process.connections[0].bytes_out = process.bytes_out;
        }
    }
    pub(super) fn terminate(
        &mut self,
        request: &crate::process::TerminationRequest,
    ) -> Result<crate::process::TerminationReport> {
        ensure!(
            !request.targets.is_empty() && request.targets.len() <= crate::process::MAX_TARGETS,
            "invalid number of captured processes"
        );
        let unique: std::collections::HashSet<_> =
            request.targets.iter().map(|target| target.pid).collect();
        ensure!(
            unique.len() == request.targets.len(),
            "duplicate process target"
        );
        ensure!(
            request.targets.iter().all(|target| self
                .snapshot
                .activity
                .iter()
                .any(|activity| activity.identities.contains(target))),
            "demo process identity changed; refresh and review again"
        );
        for activity in &mut self.snapshot.activity {
            activity
                .identities
                .retain(|identity| !request.targets.contains(identity));
            if let Some(identity) = activity.identities.first() {
                activity.pid = identity.pid;
            }
        }
        self.snapshot
            .activity
            .retain(|activity| !activity.identities.is_empty());
        Ok(crate::process::TerminationReport {
            attempted: request.targets.len(),
            delivered: request
                .targets
                .iter()
                .map(|identity| identity.pid)
                .collect(),
            failures: Vec::new(),
        })
    }
    pub(super) fn mutate(&mut self, mutation: Mutation) -> Result<()> {
        match mutation {
            Mutation::NetworkRules(rules) => {
                crate::network::validate_rules(&rules)?;
                self.snapshot.network.rules = rules;
            }
            Mutation::Setting(setting, value) => self
                .snapshot
                .firewall
                .as_mut()
                .context("missing demo firewall")?
                .set(setting, value),
            Mutation::Applications { paths, action } => {
                super::application::validate_application_targets(&paths, false)?;
                ensure!(
                    paths.iter().all(|path| self
                        .snapshot
                        .applications
                        .iter()
                        .any(|app| app.path == *path)),
                    "application is not registered; add it first"
                );
                for app in &mut self.snapshot.applications {
                    if paths.contains(&app.path) {
                        app.blocked = action == Action::Block;
                    }
                }
            }
            Mutation::AddApplication(path) => {
                validate_application_path(&path, false)?;
                ensure!(
                    self.snapshot.applications.len() < 10000,
                    "too many demo applications"
                );
                if !self
                    .snapshot
                    .applications
                    .iter()
                    .any(|app| app.path == path)
                {
                    self.snapshot.applications.push(Application {
                        name: application_name(&path),
                        path,
                        blocked: false,
                    });
                }
            }
            Mutation::RemoveApplication(path) => {
                validate_application_path(&path, false)?;
                self.snapshot.applications.retain(|app| app.path != path);
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Setting;
    #[test]
    fn demo_ticks_are_deterministic_and_poll_independent() {
        let mut a = Demo::new();
        let mut b = Demo::new();
        a.advance(Duration::from_millis(500));
        a.advance(Duration::from_secs(1));
        b.advance(Duration::from_secs(1));
        assert_eq!(a.snapshot.activity[0].bytes_in, 8000);
        assert_eq!(a.snapshot.activity[0].bytes_out, 1600);
        assert_eq!(
            a.snapshot.activity[0].bytes_in,
            b.snapshot.activity[0].bytes_in
        );
        a.mutate(Mutation::Setting(Setting::BlockAll, true))
            .unwrap();
        a.advance(Duration::from_secs(2));
        assert_eq!(a.snapshot.activity[0].bytes_in, 16000);
    }
}
