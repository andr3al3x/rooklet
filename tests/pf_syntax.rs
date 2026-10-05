use rooklet_core::{
    model::{Action, Direction, NetworkRule, Protocol},
    network::compile_rules,
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
#[cfg(target_os = "macos")]
#[test]
fn macos_pf_parser_accepts_rule_combinations_without_loading_them() {
    use std::{io::Write, process::Command};
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
    let mut input = tempfile::NamedTempFile::new().unwrap();
    input.write_all(source.as_bytes()).unwrap();
    let output = Command::new("/sbin/pfctl")
        .args(["-n", "-v", "-a", "rooklet", "-f"])
        .arg(input.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed = String::from_utf8(output.stdout).unwrap();
    assert!(parsed.contains("rooklet_matrix_0"));
    assert!(parsed.contains("rooklet_matrix_53"));
    assert!(parsed.contains(&format!("rooklet_{}", "a".repeat(55))));
}
