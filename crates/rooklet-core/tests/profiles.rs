use rooklet_core::{
    application::validate_path,
    model::{
        Action, Application, Direction, FirewallSettings, NetworkRule, NetworkStatus, Profile,
        Protocol, Setting, Snapshot,
    },
    profile,
};

fn snapshot() -> Snapshot {
    Snapshot {
        firewall: Some(Default::default()),
        applications_available: true,
        network: NetworkStatus {
            rules_available: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn network_rule() -> NetworkRule {
    NetworkRule {
        id: "service".into(),
        name: "Inbound service".into(),
        action: Action::Allow,
        destination: "192.0.2.0/24".into(),
        port: Some(443),
        protocol: Protocol::Tcp,
        direction: Direction::Inbound,
        interface: Some("en0".into()),
        enabled: true,
    }
}

#[test]
fn accepts_normalized_paths_without_filesystem_access() {
    for path in [
        "/nonexistent/App.app",
        "/Applications/With Spaces.app",
        "/usr/bin/tool",
    ] {
        validate_path(path).unwrap();
    }
    for path in [
        "",
        "/",
        "relative",
        "//App",
        "/App/",
        "/App//tool",
        "/./App",
        "/App/../tool",
        "/App\n",
    ] {
        assert!(validate_path(path).is_err(), "{path:?}");
    }
    assert!(validate_path(&format!("/{}", "a".repeat(4095))).is_err());
}

#[test]
fn choices_parse_exact_existing_spellings() {
    assert_eq!("allow".parse::<Action>().unwrap(), Action::Allow);
    assert_eq!("udp".parse::<Protocol>().unwrap(), Protocol::Udp);
    assert_eq!("in".parse::<Direction>().unwrap(), Direction::Inbound);
    assert_eq!("out".parse::<Direction>().unwrap(), Direction::Outbound);
    assert_eq!(
        "allow-signed-app".parse::<Setting>().unwrap(),
        Setting::AllowSignedApp
    );
    assert!("inbound".parse::<Direction>().is_err());
    assert!("TCP".parse::<Protocol>().is_err());
    assert!(" allow".parse::<Action>().is_err());
    assert!("allow_signed".parse::<Setting>().is_err());
}

#[test]
fn profile_roundtrip_does_not_require_registered_files() {
    let mut current = snapshot();
    current.firewall = Some(FirewallSettings {
        enabled: true,
        stealth: true,
        block_all: false,
        allow_signed: true,
        allow_signed_app: false,
    });
    current.network.rules.push(network_rule());
    current.applications.push(Application {
        path: "/nonexistent/App".into(),
        name: "App".into(),
        blocked: true,
    });
    let exported = profile::export(&current).unwrap();
    let bytes = serde_json::to_vec(&exported).unwrap();
    let parsed = profile::parse(&bytes).unwrap();
    assert_eq!(parsed.applications, current.applications);
    assert_eq!(parsed.firewall, current.firewall);
    assert_eq!(parsed.network_rules, current.network.rules);
}

#[test]
fn byte_limit_rejects_oversized_json_even_with_small_parsed_value() {
    let bytes = serde_json::to_vec(&Profile::from_snapshot(&snapshot())).unwrap();
    let mut padded = bytes.clone();
    padded.resize(profile::MAX_BYTES, b' ');
    profile::parse(&padded).unwrap();
    padded.push(b' ');
    assert!(profile::parse(&padded).is_err());
    assert!(profile::parse(b"{broken").is_err());
}

#[test]
fn strict_schema_rejects_unknown_and_missing_fields_at_each_scope() {
    let mut current = snapshot();
    current.network.rules.push(network_rule());
    current.applications.push(Application {
        path: "/App".into(),
        name: "App".into(),
        blocked: false,
    });
    let base = serde_json::to_value(Profile::from_snapshot(&current)).unwrap();
    let mut unknown_top = base.clone();
    unknown_top["unexpected"] = true.into();
    let mut unknown_firewall = base.clone();
    unknown_firewall["firewall"]["unexpected"] = true.into();
    let mut unknown_app = base.clone();
    unknown_app["applications"][0]["unexpected"] = true.into();
    let mut unknown_rule = base.clone();
    unknown_rule["network_rules"][0]["unexpected"] = true.into();
    let mut missing_firewall = base.clone();
    missing_firewall["firewall"]
        .as_object_mut()
        .unwrap()
        .remove("stealth");
    let mut missing_app = base.clone();
    missing_app["applications"][0]
        .as_object_mut()
        .unwrap()
        .remove("blocked");
    let mut missing_rule = base.clone();
    missing_rule["network_rules"][0]
        .as_object_mut()
        .unwrap()
        .remove("enabled");
    let mut missing_field = base;
    missing_field
        .as_object_mut()
        .unwrap()
        .remove("applications");
    for value in [
        unknown_top,
        unknown_firewall,
        unknown_app,
        unknown_rule,
        missing_field,
        missing_firewall,
        missing_app,
        missing_rule,
    ] {
        assert!(profile::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}

#[test]
fn validation_rejects_unsupported_scope_duplicates_and_invalid_paths() {
    let valid = Profile::from_snapshot(&snapshot());
    let mut profile = valid.clone();
    profile.version = 2;
    assert!(profile::validate(&profile).is_err());
    profile = valid.clone();
    profile.format = "foreign-profile".into();
    assert!(profile::validate(&profile).is_err());
    profile = valid.clone();
    profile.firewall = None;
    assert!(profile::validate(&profile).is_err());
    profile = valid;
    let entry = Application {
        path: "/App".into(),
        name: "App".into(),
        blocked: false,
    };
    profile.applications = vec![entry.clone(), entry];
    assert!(profile::validate(&profile).is_err());
    profile.applications.truncate(1);
    profile.applications[0].path = "/App/../other".into();
    assert!(profile::validate(&profile).is_err());
}

#[test]
fn export_rejects_unavailable_control_scopes() {
    let mut current = snapshot();
    current.firewall = None;
    assert!(profile::export(&current).is_err());
    current = snapshot();
    current.applications_available = false;
    assert!(profile::export(&current).is_err());
    current = snapshot();
    current.network.rules_available = false;
    assert!(profile::export(&current).is_err());
}
