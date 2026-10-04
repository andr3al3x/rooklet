use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};
use tempfile::tempdir;
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xield"))
}
#[test]
fn demo_status_is_explicit_and_contains_new_models() {
    let output = cli().args(["--demo", "status"]).output().unwrap();
    assert!(output.status.success());
    let data: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(data["demo"], true);
    assert!(data["activity"].as_array().unwrap().len() > 1);
    assert!(data["firewall"].is_object());
    assert!(data.get("protocol_version").is_none());
    assert!(data.get("pending").is_none());
}
#[test]
fn extension_cli_commands_and_flags_are_gone() {
    for args in [
        vec!["--bridge", "/tmp/host", "status"],
        vec!["mode", "ask"],
        vec!["allow", "code:identity"],
        vec!["import", "old.json"],
    ] {
        assert!(!cli().args(args).output().unwrap().status.success());
    }
}
#[test]
fn profile_export_roundtrips_and_never_overwrites() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("profile.json");
    assert!(
        cli()
            .args(["--demo", "profile", "export"])
            .arg(&path)
            .output()
            .unwrap()
            .status
            .success()
    );
    let before = fs::read(&path).unwrap();
    let data: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(data["format"], "xield-profile");
    assert!(
        cli()
            .args(["--demo", "profile", "check"])
            .arg(&path)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        !cli()
            .args(["--demo", "profile", "export"])
            .arg(&path)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(before, fs::read(&path).unwrap());
    assert!(
        !cli()
            .args(["--demo", "profile", "apply"])
            .arg(&path)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        cli()
            .args(["--demo", "profile", "apply"])
            .arg(&path)
            .arg("--yes")
            .output()
            .unwrap()
            .status
            .success()
    );
}
#[test]
fn profiles_reject_old_schema_and_unknown_fields() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("profile.json");
    for data in [
        json!({"version":1,"rules":[]}),
        json!({"format":"xield-profile","version":1,"firewall":{},"applications":[],"network_rules":[],"bridge":"old"}),
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
    let exported = cli()
        .args(["--demo", "profile", "export"])
        .output()
        .unwrap();
    assert!(exported.status.success());
    let mut profile: Value = serde_json::from_slice(&exported.stdout).unwrap();
    for application_path in ["/", "/Applications/../Example.app", "relative.app"] {
        profile["applications"][0]["path"] = application_path.into();
        fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();
        for command in ["check", "apply"] {
            let mut invocation = cli();
            invocation.args(["--demo", "profile", command]).arg(&path);
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
    assert!(
        !cli()
            .args(["--demo", "network", "add", "203.0.113.1", "--port", "0"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let dir = tempdir().unwrap();
    let path = dir.path().join("rules.json");
    fs::write(&path, "[{\"app_id\":\"old\"}]").unwrap();
    assert!(
        !cli()
            .args(["network", "preview"])
            .arg(&path)
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn country_update_help_replaces_the_old_path_flag() {
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

#[test]
fn demo_country_update_downloads_nothing_and_creates_no_files() {
    let home = tempdir().unwrap();
    let output = cli()
        .args(["--demo", "geoip", "update"])
        .env("HOME", home.path())
        .env("XIELD_GEOIP", "/nonexistent/obsolete.mmdb")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("No download or files changed"));
    assert_eq!(fs::read_dir(home.path()).unwrap().count(), 0);
}
