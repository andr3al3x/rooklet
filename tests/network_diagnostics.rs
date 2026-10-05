use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};
use xield::model::{Action, Direction, NetworkRule, Protocol};

fn rules() -> Vec<NetworkRule> {
    vec![
        NetworkRule {
            id: "first".into(),
            name: "First".into(),
            destination: "203.0.113.0/24".into(),
            port: Some(443),
            protocol: Protocol::Tcp,
            direction: Direction::Outbound,
            action: Action::Block,
            interface: Some("en0".into()),
            enabled: true,
        },
        NetworkRule {
            id: "second".into(),
            name: "Second".into(),
            destination: "203.0.113.5".into(),
            port: Some(443),
            protocol: Protocol::Tcp,
            direction: Direction::Outbound,
            action: Action::Allow,
            interface: Some("en0".into()),
            enabled: true,
        },
    ]
}
fn cli(args: &[&str], data: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_xield"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(data).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn supplied_rules_check_without_demo_or_system_access_and_report_shadowing() {
    let data = serde_json::to_vec(&rules()).unwrap();
    let output = cli(&["network", "check", "--stdin"], &data);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["rule_count"], 2);
    assert_eq!(report["warnings"][0]["shadowed_id"], "second");
    assert_eq!(report["warnings"][0]["covering_position"], 1);
    assert!(
        report["scope"]
            .as_str()
            .unwrap()
            .contains("not a live verdict")
    );
}

#[test]
fn explanation_keeps_missing_fields_unknown_and_handles_file_input() {
    let data = serde_json::to_vec(&rules()).unwrap();
    let args = ["network", "explain", "--stdin", "--remote", "203.0.113.5"];
    let output = cli(&args, &data);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["explanation"]["result"], "indeterminate");
    assert_eq!(
        report["explanation"]["missing_fields"],
        json!(["destination_port", "interface"])
    );
    assert!(report["query"]["destination_port"].is_null());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rules with spaces.json");
    std::fs::write(&path, data).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xield"))
        .args(["network", "explain"])
        .arg(&path)
        .args([
            "--remote",
            "203.0.113.5",
            "--port",
            "443",
            "--interface",
            "en0",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["explanation"]["result"], "matched");
    assert_eq!(report["explanation"]["rule_id"], "first");
    assert_eq!(report["explanation"]["action"], "block");
}

#[test]
fn diagnostics_reject_bad_rules_and_non_concrete_queries() {
    let data = serde_json::to_vec(&rules()).unwrap();
    for option in [
        vec!["--protocol", "any"],
        vec!["--direction", "both"],
        vec!["--port", "0"],
        vec!["--interface", "bad interface"],
    ] {
        let mut args = vec!["network", "explain", "--stdin", "--remote", "203.0.113.5"];
        args.extend(option);
        assert!(!cli(&args, &data).status.success());
    }
    for data in [b"{}".as_slice(), b"[{}]".as_slice(), b"garbage".as_slice()] {
        assert!(!cli(&["network", "check", "--stdin"], data).status.success());
    }
    assert!(cli(&["--demo", "network", "check"], b"").status.success());
}

#[test]
fn demo_apply_warns_before_output_but_does_not_reject_valid_shadowed_rules() {
    let output = cli(
        &["--demo", "network", "apply", "--stdin"],
        &serde_json::to_vec(&rules()).unwrap(),
    );
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("second is fully shadowed by #1 first")
    );
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["rules"].as_array().unwrap().len(), 2);
}
