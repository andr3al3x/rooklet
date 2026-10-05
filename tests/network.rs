use rooklet::{
    model::{Action, Direction, NetworkRule, Protocol},
    network::{compile_rules, validate_rules},
};

fn rule() -> NetworkRule {
    NetworkRule {
        id: "first-rule".into(),
        name: "A network rule".into(),
        action: Action::Block,
        destination: "192.0.2.42/24".into(),
        port: Some(443),
        protocol: Protocol::Tcp,
        direction: Direction::Both,
        interface: Some("en0".into()),
        enabled: true,
    }
}
#[test]
fn cidr_is_canonical_and_both_directions_match_the_remote_peer() {
    let compiled = compile_rules(&[rule()]).unwrap();
    assert!(compiled.contains("block drop in quick on en0 inet proto tcp from 192.0.2.0/24 to any port 443 label \"rooklet_first-rule\""));
    assert!(compiled.contains("block drop out quick on en0 inet proto tcp from any to 192.0.2.0/24 port 443 label \"rooklet_first-rule\""));
}
#[test]
fn ipv6_and_any_are_explicit_and_disabled_rules_are_omitted() {
    let mut ipv6 = rule();
    ipv6.destination = "2001:db8::1".into();
    ipv6.direction = Direction::Outbound;
    ipv6.port = None;
    ipv6.protocol = Protocol::Any;
    ipv6.action = Action::Allow;
    let compiled = compile_rules(&[ipv6.clone()]).unwrap();
    assert!(compiled.contains("out quick on en0 inet6 from any to 2001:db8::1/128 keep state"));
    assert!(!compiled.contains(" proto "));
    ipv6.destination = "any".into();
    let compiled = compile_rules(&[ipv6.clone()]).unwrap();
    assert_eq!(
        compiled.lines().filter(|l| l.starts_with("pass ")).count(),
        2
    );
    ipv6.enabled = false;
    assert_eq!(compile_rules(&[ipv6]).unwrap().lines().count(), 1);
}
#[test]
fn rule_order_is_stable_and_names_never_enter_pf_source() {
    let mut first = rule();
    first.name = "quote \" and # braces { }".into();
    let mut second = first.clone();
    second.id = "second".into();
    second.action = Action::Allow;
    let compiled = compile_rules(&[first, second]).unwrap();
    assert!(!compiled.contains("quote"));
    assert!(
        compiled.find("rooklet_first-rule").unwrap() < compiled.find("rooklet_second").unwrap()
    );
    assert!(compiled.lines().skip(1).all(|l| l.contains(" quick ")));
}
#[test]
fn malicious_fields_and_invalid_ports_are_rejected_even_when_disabled() {
    for destination in [
        "any\npass all",
        "example.com",
        "{ 1.2.3.4 }",
        "1.2.3.4/33",
        "::/129",
        " 1.2.3.4",
    ] {
        let mut r = rule();
        r.destination = destination.into();
        r.enabled = false;
        assert!(validate_rules(&[r]).is_err(), "{destination}");
    }
    for id in ["", "foo\"", "a\nb", "../x", "a b"] {
        let mut r = rule();
        r.id = id.into();
        assert!(validate_rules(&[r]).is_err(), "{id}");
    }
    for interface in ["en0\npass all", "(en0)", "!en0", "en0:network", "", "1en"] {
        let mut r = rule();
        r.interface = Some(interface.into());
        assert!(validate_rules(&[r]).is_err(), "{interface}");
    }
    let mut r = rule();
    r.port = Some(0);
    assert!(validate_rules(&[r.clone()]).is_err());
    r.port = Some(80);
    r.protocol = Protocol::Any;
    assert!(validate_rules(&[r]).is_err());
    assert!(validate_rules(&[rule(), rule()]).is_err());
}

#[test]
fn rule_id_boundary_keeps_namespaced_labels_within_pf_limit() {
    let mut boundary = rule();
    boundary.id = "a".repeat(55);
    validate_rules(&[boundary.clone()]).unwrap();
    let source = compile_rules(&[boundary.clone()]).unwrap();
    assert!(source.contains(&format!("label \"rooklet_{}\"", boundary.id)));
    assert_eq!(format!("rooklet_{}", boundary.id).len(), 63);
    boundary.id.push('a');
    boundary.enabled = false;
    let error = validate_rules(&[boundary.clone()]).unwrap_err();
    assert!(error.to_string().contains("1–55 characters"));
    assert!(compile_rules(&[boundary]).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn macos_pf_parser_accepts_rule_combinations_without_loading_them() {
    use std::{path::Path, sync::atomic::AtomicBool};
    let mut rules = Vec::new();
    for action in [Action::Allow, Action::Block] {
        for protocol in [Protocol::Any, Protocol::Tcp, Protocol::Udp] {
            for direction in [Direction::Both, Direction::Inbound, Direction::Outbound] {
                for destination in ["any", "192.0.2.0/24", "2001:db8::/32"] {
                    let mut r = rule();
                    r.id = format!("matrix_{}", rules.len());
                    r.action = action;
                    r.protocol = protocol;
                    r.direction = direction;
                    r.destination = destination.into();
                    r.interface = None;
                    r.port = if protocol == Protocol::Any {
                        None
                    } else {
                        Some(443)
                    };
                    rules.push(r);
                }
            }
        }
    }
    let mut boundary = rule();
    boundary.id = "a".repeat(55);
    boundary.interface = None;
    rules.push(boundary);
    let source = compile_rules(&rules).unwrap();
    // -n only parses; this test never loads PF rules, enables PF, or uses sudo.
    let parsed = rooklet::command::run(
        Path::new("/sbin/pfctl"),
        &["-n", "-v", "-a", "rooklet", "-f", "-"].map(String::from),
        Some(source.as_bytes()),
        false,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(parsed.contains("rooklet_matrix_0"));
    assert!(parsed.contains("rooklet_matrix_53"));
    assert!(parsed.contains(&format!("rooklet_{}", "a".repeat(55))));
}
