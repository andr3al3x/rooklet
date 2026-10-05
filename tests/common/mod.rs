//! Static model fixtures for interaction tests; never invoke system adapters.
use xield::model::{
    Action, Application, Connection, Country, Direction, FirewallSettings, NetworkRule,
    NetworkStatus, ProcessActivity, Protocol, Snapshot,
};

pub fn snapshot() -> Snapshot {
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
            identities: vec![xield::process::ProcessIdentity {
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
        id: "fixture-updater".into(),
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
        message: Some("Fixture network rules".into()),
    };
    let notices = Vec::new();
    Snapshot {
        applications_available: true,
        firewall: Some(firewall),
        applications,
        activity,
        network,
        geoip: Some("DB-IP Lite · fixture country data · CC BY 4.0".into()),
        notices,
    }
}
