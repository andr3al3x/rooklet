//! File diagnostics must not change CLI output or expose configuration data.
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tempfile::tempdir;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rooklet"))
}

fn records(directory: &Path) -> Vec<Value> {
    let files: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .collect();
    assert_eq!(files.len(), 1);
    fs::read_to_string(&files[0])
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn successful_cli_keeps_json_and_flushes_private_diagnostics() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("private-profile-name.json");
    let log_dir = directory.path().join("logs");
    let profile = json!({
        "format": "rooklet-profile", "version": 1,
        "firewall": {"enabled": true, "stealth": false, "block_all": false,
                     "allow_signed": true, "allow_signed_app": true},
        "applications": [{"path": "/private-sensitive-app.app", "name": "private-sensitive-label", "blocked": true}],
        "network_rules": []
    });
    fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();
    let output = cli()
        .args(["profile", "check"])
        .arg(&path)
        .arg("--log-dir")
        .arg(&log_dir)
        .args(["--log-level", "trace"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        profile
    );
    let records = records(&log_dir);
    assert!(
        records
            .iter()
            .any(|record| record["fields"]["message"] == "session completed")
    );
    assert!(records.iter().any(|record| {
        record["fields"]["message"] == "operation completed"
            && record["span"]["operation"] == "profile.check"
            && record["span"]["operation_id"] == 1
    }));
    assert_eq!(
        records.last().unwrap()["fields"]["event"],
        "logging_finished"
    );
    let logs = serde_json::to_string(&records).unwrap();
    assert!(!logs.contains("private-sensitive"));
    assert!(!logs.contains("private-profile-name"));
    assert!(!logs.contains(&path.to_string_lossy().to_string()));
}

#[test]
fn failed_cli_preserves_exit_code_and_filters_severity_without_error_payloads() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("sensitive-missing-profile.json");
    let log_dir = directory.path().join("logs");
    let output = cli()
        .args(["--log-level", "error", "--log-dir"])
        .arg(&log_dir)
        .args(["profile", "check"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("sensitive-missing-profile"));
    let records = records(&log_dir);
    let events: Vec<_> = records
        .iter()
        .filter(|record| record["fields"]["event"] != "logging_finished")
        .collect();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["level"], "ERROR");
    assert_eq!(events[0]["fields"]["operation"], "profile.check");
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("sensitive-missing-profile")
    );
    assert_eq!(
        records.last().unwrap()["fields"]["event"],
        "logging_finished"
    );
}

#[test]
fn parse_errors_and_help_never_start_the_sink() {
    let directory = tempdir().unwrap();
    let log_dir = directory.path().join("logs");
    for (arguments, code) in [(["--help"].as_slice(), 0), (["--unknown"].as_slice(), 2)] {
        let output = cli()
            .arg("--log-dir")
            .arg(&log_dir)
            .args(arguments)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(code));
        assert!(!log_dir.exists());
    }
    let output = cli()
        .args(["--log-level", "debug", "status"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn logging_is_disabled_by_default() {
    let directory = tempdir().unwrap();
    let output = cli()
        .current_dir(directory.path())
        .args(["profile", "check", "missing.json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}
