use rooklet::{clean, clean_multiline};
use serde_json::{Value, json};
use std::{fs, process::Command};
use tempfile::tempdir;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rooklet"))
}

#[test]
fn multiline_diagnostics_preserve_lines_without_terminal_or_bidi_controls() {
    let text = "first\r\nsecond\t\x1b[2J\u{061c}\u{200e}\u{200f}\u{202e}\u{2066}\u{206f}last";
    assert_eq!(clean_multiline(text), "first\nsecond[2Jlast");
    assert_eq!(clean(text), "firstsecond[2Jlast");
}

#[test]
fn missing_profile_path_cannot_inject_terminal_controls_into_errors() {
    let dir = tempdir().unwrap();
    let path = dir
        .path()
        .join("missing-\x1b[2J\r\u{202e}\u{2066}profile.json");
    let output = cli()
        .args(["profile", "check"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).unwrap();
    assert!(
        diagnostic.contains("missing-[2Jprofile.json"),
        "{diagnostic}"
    );
    assert_eq!(diagnostic, clean_multiline(&diagnostic));
}

#[test]
fn argument_errors_and_help_keep_their_exit_codes_and_output_streams() {
    let output = cli()
        .args(["--theme", "bad\x1b[2J\u{202e}"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).unwrap();
    assert!(diagnostic.contains("invalid value 'bad"), "{diagnostic}");
    assert_eq!(diagnostic, clean_multiline(&diagnostic));

    let output = cli().arg("--help").output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("\nUsage:"));
    assert!(help.contains("\nCommands:"));
}

#[test]
fn human_sanitization_does_not_change_json_profile_fields() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("profile.json");
    let profile = json!({
        "format": "rooklet-profile", "version": 1,
        "firewall": {"enabled": true, "stealth": false, "block_all": false,
                     "allow_signed": true, "allow_signed_app": true},
        "applications": [{"path": "/nonexistent/App.app", "name": "App\u{202e}\u{2066}", "blocked": true}],
        "network_rules": []
    });
    fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();
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
}
