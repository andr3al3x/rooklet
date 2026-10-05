use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

use clap::Parser;
use serde_json::Value;

use super::{Level, Options, directory, sink, start};

#[derive(Parser)]
struct Arguments {
    #[command(flatten)]
    logging: Options,
}

fn records(path: &std::path::Path) -> Vec<Value> {
    let files = directory::run_files(path);
    assert_eq!(files.len(), 1);
    let bytes = fs::read(&files[0]).unwrap();
    assert!(!bytes.contains(&0x1b), "log contains ANSI escape bytes");
    String::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("complete JSONL record"))
        .collect()
}

#[test]
fn logging_is_optional_and_explicit_level_requires_directory() {
    let arguments = Arguments::try_parse_from(["rooklet"]).unwrap();
    assert!(arguments.logging.log_dir.is_none());
    let session = arguments.logging.start().unwrap();
    assert!(session.worker.is_none());
    session.finish();
    assert!(Arguments::try_parse_from(["rooklet", "--log-level", "debug"]).is_err());
    assert!(
        Arguments::try_parse_from(["rooklet", "--log-dir", "/unused", "--log-level", "trace"])
            .is_ok()
    );
}

#[test]
fn filters_level_and_dependency_targets_and_flushes_on_finish() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("logs");
    let (session, dispatch) = start(&path, Level::Info, sink::Limits::default()).unwrap();
    tracing::dispatcher::with_default(&dispatch, || {
        tracing::debug!(target: "rooklet::test", event = "hidden_debug");
        tracing::info!(target: "dependency", event = "hidden_dependency");
        tracing::info!(target: "rooklet::test", event = "accepted_info");
        tracing::warn!(target: "rooklet_macos::test", event = "accepted_warn");
    });
    session.finish();
    let records = records(&path);
    let events: Vec<_> = records
        .iter()
        .map(|record| record["fields"]["event"].as_str().unwrap())
        .collect();
    assert_eq!(
        events,
        ["accepted_info", "accepted_warn", "logging_finished"]
    );
    assert_eq!(records.last().unwrap()["fields"]["queue_dropped"], 0);
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o700);
    assert_eq!(
        fs::metadata(&directory::run_files(&path)[0])
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn preserves_operation_span_fields() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("logs");
    let (session, dispatch) = start(&path, Level::Info, sink::Limits::default()).unwrap();
    tracing::dispatcher::with_default(&dispatch, || {
        let operation = tracing::info_span!(target: "rooklet::test", "operation", operation_id = 42, kind = "refresh");
        let _entered = operation.enter();
        tracing::info!(target: "rooklet::test", event = "operation_finished");
    });
    session.finish();
    let records = records(&path);
    assert_eq!(records[0]["span"]["operation_id"], 42);
    assert_eq!(records[0]["spans"][0]["kind"], "refresh");
}

#[test]
fn concurrent_sessions_share_directory_and_keep_retention_bounded() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("logs");
    let (first, _) = start(&path, Level::Info, sink::Limits::default()).unwrap();
    let (second, _) = start(&path, Level::Debug, sink::Limits::default()).unwrap();
    assert_eq!(directory::run_files(&path).len(), 2);
    first.finish();
    second.finish();
}

#[test]
fn record_and_file_caps_preserve_json_and_report_loss() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("logs");
    let limits = sink::Limits {
        record_bytes: 512,
        file_bytes: 2048,
        ..sink::Limits::default()
    };
    let (session, dispatch) = start(&path, Level::Trace, limits).unwrap();
    tracing::dispatcher::with_default(&dispatch, || {
        let oversized = "x".repeat(1000);
        tracing::info!(target: "rooklet::test", event = "oversized", detail = oversized);
        for index in 0..50 {
            tracing::info!(target: "rooklet::test", event = "bounded", index);
        }
    });
    session.finish();
    let files = directory::run_files(&path);
    assert!(fs::metadata(&files[0]).unwrap().len() <= limits.file_bytes as u64);
    let records = records(&path);
    let summary = &records.last().unwrap()["fields"];
    assert_eq!(summary["oversized_records"], 1);
    assert!(summary["file_budget_dropped"].as_u64().unwrap() > 0);
    assert!(
        records
            .iter()
            .all(|record| record["fields"]["event"] != "oversized")
    );
}

#[test]
fn drop_flushes_and_retention_preserves_unrelated_files() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("logs");
    for index in 0..7 {
        let (session, dispatch) = start(&path, Level::Info, sink::Limits::default()).unwrap();
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::info!(target: "rooklet::test", event = "retained", index);
        });
        drop(session);
        fs::write(path.join("notes.txt"), "keep this").unwrap();
    }
    let files = directory::run_files(&path);
    assert_eq!(files.len(), 5);
    assert_eq!(
        fs::read_to_string(path.join("notes.txt")).unwrap(),
        "keep this"
    );
    for (path, expected_index) in files.iter().zip(2..7) {
        let first_line = fs::read_to_string(path)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .to_owned();
        let first: Value = serde_json::from_str(&first_line).unwrap();
        assert_eq!(first["fields"]["index"], expected_index);
    }
}

#[test]
fn refuses_symlink_or_public_directory_without_modifying_it() {
    let temporary = tempfile::tempdir().unwrap();
    let target = temporary.path().join("target");
    fs::create_dir(&target).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
    let link = temporary.path().join("link");
    symlink(&target, &link).unwrap();
    assert!(start(&link, Level::Info, sink::Limits::default()).is_err());
    let trailing_slash = std::path::PathBuf::from(format!("{}/", link.display()));
    assert!(start(&trailing_slash, Level::Info, sink::Limits::default()).is_err());
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(start(&target, Level::Info, sink::Limits::default()).is_err());
    assert_eq!(fs::metadata(&target).unwrap().mode() & 0o777, 0o755);
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
}

#[test]
fn refuses_linked_run_file_and_preserves_its_target() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("logs");
    drop(directory::create_run(&path, 5).unwrap());
    let file = directory::run_files(&path).pop().unwrap();
    fs::remove_file(&file).unwrap();
    let target = temporary.path().join("secret");
    fs::write(&target, "keep").unwrap();
    symlink(&target, &file).unwrap();
    assert!(start(&path, Level::Info, sink::Limits::default()).is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "keep");
    assert!(fs::symlink_metadata(file).unwrap().file_type().is_symlink());
}

#[test]
fn refuses_linked_directory_lock_and_preserves_its_target() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("logs");
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let target = temporary.path().join("secret");
    fs::write(&target, "keep").unwrap();
    symlink(&target, path.join(".rooklet.lock")).unwrap();
    assert!(start(&path, Level::Info, sink::Limits::default()).is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "keep");
    assert!(directory::run_files(&path).is_empty());
}
