use rooklet_core::{
    model::{Action, Direction, NetworkRule, Protocol},
    network::{QueryField, RuleExplanation, RuleQuery, explain_rules, shadow_warnings},
};

fn rule(id: &str, destination: &str) -> NetworkRule {
    NetworkRule {
        id: id.into(),
        name: id.into(),
        action: Action::Block,
        destination: destination.into(),
        port: None,
        protocol: Protocol::Any,
        direction: Direction::Both,
        interface: None,
        enabled: true,
    }
}

fn query(remote: &str) -> RuleQuery {
    RuleQuery {
        remote_ip: remote.parse().unwrap(),
        protocol: Protocol::Tcp,
        direction: Direction::Outbound,
        destination_port: Some(443),
        interface: Some("en0".into()),
    }
}

fn matched(id: &str, position: usize, action: Action) -> RuleExplanation {
    RuleExplanation::Matched {
        rule_id: id.into(),
        position,
        action,
    }
}

#[test]
fn quick_order_and_disabled_rules_preserve_original_positions() {
    let mut disabled = rule("disabled", "any");
    disabled.enabled = false;
    let mut allow = rule("allow", "192.0.2.0/24");
    allow.action = Action::Allow;
    let block = rule("block", "any");
    let rules = [disabled, allow, block];
    assert_eq!(
        explain_rules(&rules, &query("192.0.2.42")).unwrap(),
        matched("allow", 2, Action::Allow)
    );
    assert_eq!(
        explain_rules(&rules, &query("198.51.100.1")).unwrap(),
        matched("block", 3, Action::Block)
    );
    let reordered = [rules[2].clone(), rules[1].clone()];
    assert_eq!(
        explain_rules(&reordered, &query("192.0.2.42")).unwrap(),
        matched("block", 1, Action::Block)
    );
}

#[test]
fn inbound_matches_remote_source_and_local_destination_service() {
    let mut inbound = rule("service", "192.0.2.42");
    inbound.direction = Direction::Inbound;
    inbound.protocol = Protocol::Tcp;
    inbound.port = Some(22);
    let mut packet = query("192.0.2.42");
    let rules = [inbound];
    assert_eq!(
        explain_rules(&rules, &packet).unwrap(),
        RuleExplanation::NoMatch
    );
    packet.direction = Direction::Inbound;
    assert_eq!(
        explain_rules(&rules, &packet).unwrap(),
        RuleExplanation::NoMatch
    );
    packet.destination_port = Some(22);
    assert_eq!(
        explain_rules(&rules, &packet).unwrap(),
        matched("service", 1, Action::Block)
    );
    packet.remote_ip = "192.0.2.43".parse().unwrap();
    assert_eq!(
        explain_rules(&rules, &packet).unwrap(),
        RuleExplanation::NoMatch
    );
}

#[test]
fn unknown_fields_block_a_later_definite_match_only_when_earlier_rule_is_possible() {
    let mut scoped = rule("scoped", "192.0.2.0/24");
    scoped.protocol = Protocol::Tcp;
    scoped.port = Some(443);
    scoped.interface = Some("en0".into());
    let rules = [scoped, rule("fallback", "any")];
    let mut packet = query("192.0.2.1");
    packet.destination_port = None;
    packet.interface = None;
    assert_eq!(
        explain_rules(&rules, &packet).unwrap(),
        RuleExplanation::Indeterminate {
            rule_id: "scoped".into(),
            position: 1,
            missing_fields: vec![QueryField::DestinationPort, QueryField::Interface],
        }
    );
    packet.protocol = Protocol::Udp;
    assert_eq!(
        explain_rules(&rules, &packet).unwrap(),
        matched("fallback", 2, Action::Block)
    );
    packet.protocol = Protocol::Tcp;
    packet.destination_port = Some(80);
    assert_eq!(
        explain_rules(&rules, &packet).unwrap(),
        matched("fallback", 2, Action::Block)
    );
    packet.destination_port = None;
    packet.interface = Some("en1".into());
    assert_eq!(
        explain_rules(&rules, &packet).unwrap(),
        matched("fallback", 2, Action::Block)
    );
    packet.interface = None;
    packet.remote_ip = "2001:db8::1".parse().unwrap();
    assert_eq!(
        explain_rules(&rules, &packet).unwrap(),
        matched("fallback", 2, Action::Block)
    );
}

#[test]
fn cidr_host_bits_ipv6_and_address_families_match_correctly() {
    let rules = [
        rule("v4", "192.0.2.99/24"),
        rule("v6", "2001:db8:12::99/48"),
    ];
    assert_eq!(
        explain_rules(&rules, &query("192.0.2.1")).unwrap(),
        matched("v4", 1, Action::Block)
    );
    assert_eq!(
        explain_rules(&rules, &query("2001:db8:12::abcd")).unwrap(),
        matched("v6", 2, Action::Block)
    );
    assert_eq!(
        explain_rules(&rules, &query("2001:db8:13::1")).unwrap(),
        RuleExplanation::NoMatch
    );
}

#[test]
fn shadowing_requires_complete_address_coverage_not_overlap() {
    let rules = [
        rule("v4", "192.0.2.99/24"),
        rule("host", "192.0.2.100"),
        rule("wider", "192.0.0.0/16"),
        rule("v6", "2001:db8::/32"),
        rule("v6-host", "2001:db8::1"),
        rule("any", "any"),
    ];
    let warnings = shadow_warnings(&rules).unwrap();
    assert_eq!(warnings.len(), 2);
    assert_eq!(
        (&warnings[0].shadowed_id, &warnings[0].covering_id),
        (&"host".to_string(), &"v4".to_string())
    );
    assert_eq!(
        (warnings[0].shadowed_position, warnings[0].covering_position),
        (2, 1)
    );
    assert_eq!(
        (&warnings[1].shadowed_id, &warnings[1].covering_id),
        (&"v6-host".to_string(), &"v6".to_string())
    );
}

#[test]
fn duplicate_semantics_with_different_ids_and_actions_are_shadowed() {
    let first = rule("first", "any");
    let mut duplicate = rule("duplicate", "any");
    duplicate.action = Action::Allow;
    let warnings = shadow_warnings(&[first, duplicate]).unwrap();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].covering_id, "first");
}

#[test]
fn address_family_wildcards_cover_only_their_family() {
    let rules = [
        rule("v4-all", "0.0.0.0/0"),
        rule("v6-all", "::/0"),
        rule("v4-host", "203.0.113.1"),
        rule("v6-host", "2001:db8::1"),
        rule("any", "any"),
    ];
    let warnings = shadow_warnings(&rules).unwrap();
    assert_eq!(warnings.len(), 2);
    assert_eq!(warnings[0].covering_id, "v4-all");
    assert_eq!(warnings[1].covering_id, "v6-all");
    assert_eq!(
        explain_rules(&rules, &query("2001:db8::1")).unwrap(),
        matched("v6-all", 2, Action::Block)
    );
}

#[test]
fn narrower_scopes_and_disabled_rules_do_not_shadow_broader_rules() {
    let mut first = rule("scoped", "any");
    first.protocol = Protocol::Tcp;
    first.direction = Direction::Outbound;
    first.port = Some(443);
    first.interface = Some("en0".into());
    assert!(
        shadow_warnings(&[first.clone(), rule("broad", "any")])
            .unwrap()
            .is_empty()
    );
    for (id, change) in [
        ("protocol", 0),
        ("direction", 1),
        ("port", 2),
        ("interface", 3),
        ("disabled", 4),
    ] {
        let mut later = first.clone();
        later.id = id.into();
        match change {
            0 => later.protocol = Protocol::Udp,
            1 => later.direction = Direction::Inbound,
            2 => later.port = Some(80),
            3 => later.interface = Some("en1".into()),
            _ => later.enabled = false,
        }
        assert!(shadow_warnings(&[first.clone(), later]).unwrap().is_empty());
    }
    let mut disabled = rule("disabled", "any");
    disabled.enabled = false;
    assert!(shadow_warnings(&[disabled, first]).unwrap().is_empty());
}

#[test]
fn broader_scopes_cover_scoped_rules_but_unions_are_not_inferred() {
    let mut later = rule("scoped", "2001:db8::1");
    later.protocol = Protocol::Udp;
    later.direction = Direction::Inbound;
    later.port = Some(53);
    later.interface = Some("en0".into());
    assert_eq!(
        shadow_warnings(&[rule("any", "any"), later]).unwrap().len(),
        1
    );
    let halves = [
        rule("lower", "192.0.2.0/25"),
        rule("upper", "192.0.2.128/25"),
        rule("whole", "192.0.2.0/24"),
    ];
    assert!(shadow_warnings(&halves).unwrap().is_empty());
}

#[test]
fn invalid_rules_and_non_concrete_queries_are_rejected() {
    let rules = [rule("valid", "any")];
    let mut packet = query("192.0.2.1");
    packet.protocol = Protocol::Any;
    assert!(explain_rules(&rules, &packet).is_err());
    packet.protocol = Protocol::Tcp;
    packet.direction = Direction::Both;
    assert!(explain_rules(&rules, &packet).is_err());
    packet.direction = Direction::Outbound;
    packet.destination_port = Some(0);
    assert!(explain_rules(&rules, &packet).is_err());
    packet.destination_port = None;
    packet.interface = Some("en0\n".into());
    assert!(explain_rules(&rules, &packet).is_err());
    let bad = [rule("bad", "not an address")];
    assert!(shadow_warnings(&bad).is_err());
    assert!(explain_rules(&bad, &query("192.0.2.1")).is_err());
}
