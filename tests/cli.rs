use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};
use tempfile::tempdir;
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rooklet"))
}

#[test]
fn incoming_helper_is_hidden_and_cannot_start_diagnostics() {
    let help = cli().arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(!String::from_utf8_lossy(&help.stdout).contains("__incoming"));
    let directory = tempdir().unwrap();
    let log_dir = directory.path().join("logs");
    let output = cli()
        .args(["__incoming", "--log-dir"])
        .arg(&log_dir)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not accept logging options"));
    assert!(!log_dir.exists());
    assert!(output.stdout.is_empty());
}

#[test]
fn incoming_helper_rejects_unprivileged_invocation_before_reading_stdin() {
    if rooklet_macos::is_root() {
        return;
    }
    // Keep stdin open and send no bytes. Root enforcement must precede input.
    let mut child = cli()
        .arg("__incoming")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("unprivileged helper waited for input");
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires root"));
    assert!(output.stdout.is_empty());
}
#[test]
fn unsupported_commands_and_flags_are_rejected() {
    for args in [
        vec!["--bridge", "/tmp/host", "status"],
        vec!["mode", "ask"],
        vec!["allow", "code:identity"],
        vec!["import", "profile.json"],
    ] {
        assert!(!cli().args(args).output().unwrap().status.success());
    }
}
fn supplied_profile() -> Value {
    json!({
        "format": "rooklet-profile", "version": 1,
        "firewall": {"enabled": true, "stealth": false, "block_all": false,
                     "allow_signed": true, "allow_signed_app": true},
        "applications": [{"path": "/nonexistent/App.app", "name": "App", "blocked": true}],
        "network_rules": []
    })
}
#[test]
fn supplied_profile_check_roundtrips_without_changing_input() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("profile.json");
    let profile = supplied_profile();
    let before = serde_json::to_vec(&profile).unwrap();
    fs::write(&path, &before).unwrap();
    let output = cli()
        .args(["profile", "check"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        profile
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    let output = cli()
        .args(["profile", "apply"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("pass --yes"));
}
#[test]
fn profiles_reject_invalid_schemas_and_unknown_fields() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("profile.json");
    let mut unknown_field = supplied_profile();
    unknown_field["unsupported"] = true.into();
    let mut unknown_format = supplied_profile();
    unknown_format["format"] = "foreign-profile".into();
    let mut unknown_version = supplied_profile();
    unknown_version["version"] = 2.into();
    for data in [
        json!({"version":1,"rules":[]}),
        unknown_field,
        unknown_format,
        unknown_version,
    ] {
        fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
        assert!(
            !cli()
                .args(["profile", "check"])
                .arg(&path)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
}

#[test]
fn profiles_reject_non_normalized_paths_before_apply() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("profile.json");
    let mut profile = supplied_profile();
    for application_path in ["/", "/Applications/../Example.app", "relative.app"] {
        profile["applications"][0]["path"] = application_path.into();
        fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();
        for command in ["check", "apply"] {
            let mut invocation = cli();
            invocation.args(["profile", command]).arg(&path);
            if command == "apply" {
                invocation.arg("--yes");
            }
            let output = invocation.output().unwrap();
            assert!(!output.status.success(), "accepted {application_path}");
            assert!(String::from_utf8_lossy(&output.stderr).contains("absolute normalized path"));
        }
    }
}
#[test]
fn preview_reads_stdin_without_applying_pf() {
    let rule = json!({"id":"example","name":"Example","action":"block","destination":"203.0.113.0/24","port":443,"protocol":"tcp","direction":"out","interface":null,"enabled":true});
    let mut child = cli()
        .args(["network", "preview", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(serde_json::to_string(&vec![rule]).unwrap().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("block drop out quick inet proto tcp from any to 203.0.113.0/24 port 443")
    );
}
#[test]
fn invalid_rules_and_ports_fail_before_authentication() {
    let output = cli()
        .args(["network", "add", "203.0.113.1", "--port", "0"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).unwrap();
    assert!(diagnostic.contains("invalid value '0'"), "{diagnostic}");
    assert!(diagnostic.contains("--port"), "{diagnostic}");
    let dir = tempdir().unwrap();
    let path = dir.path().join("rules.json");
    fs::write(&path, "[{\"unsupported\":true}]").unwrap();
    let output = cli()
        .args(["network", "preview"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).unwrap();
    assert!(
        diagnostic.contains("expected a JSON array of network rules"),
        "{diagnostic}"
    );
    assert!(
        diagnostic.contains("unknown field `unsupported`"),
        "{diagnostic}"
    );
}

#[test]
fn country_update_help_documents_managed_database_updates() {
    let help = cli().arg("--help").output().unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("geoip"));
    assert!(!help.contains("--geoip"));
    let command_help = cli().args(["geoip", "update", "--help"]).output().unwrap();
    assert!(command_help.status.success());
    assert!(String::from_utf8_lossy(&command_help.stdout).contains("DB-IP"));
    assert!(
        !cli()
            .args(["--geoip", "/tmp/country.mmdb", "status"])
            .output()
            .unwrap()
            .status
            .success()
    );
}
