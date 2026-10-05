//! Harmless helper fixtures exercise transaction supervision without PF or sudo.
use crate::command;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[test]
fn transaction_checks_cancellation_before_launch() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("launched");
    let error = command::run_transaction(
        Path::new("/bin/sh"),
        &[
            "-c".into(),
            "printf launched > \"$1\"".into(),
            "fixture".into(),
            marker.to_str().unwrap().into(),
        ],
        None,
        false,
        &AtomicBool::new(true),
    )
    .unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    assert!(!marker.exists());
}

#[test]
fn transaction_finishes_after_cancellation_during_helper() {
    let directory = tempfile::tempdir().unwrap();
    let ready = directory.path().join("ready");
    let complete = directory.path().join("complete");
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&cancel);
    let ready_for_thread = ready.clone();
    let thread = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready_for_thread.exists() {
            assert!(Instant::now() < deadline, "fixture did not launch");
            std::thread::sleep(Duration::from_millis(5));
        }
        signal.store(true, Ordering::Relaxed);
    });
    let output = command::run_transaction(
        Path::new("/bin/sh"),
        &[
            "-c".into(),
            "printf ready > \"$1\"; /bin/sleep 0.2; printf restored > \"$2\"; printf finished"
                .into(),
            "fixture".into(),
            ready.to_str().unwrap().into(),
            complete.to_str().unwrap().into(),
        ],
        None,
        false,
        &cancel,
    )
    .unwrap();
    thread.join().unwrap();
    assert!(cancel.load(Ordering::Relaxed));
    assert_eq!(output, "finished");
    assert_eq!(std::fs::read_to_string(complete).unwrap(), "restored");
}

#[test]
fn transaction_discards_excess_output_but_waits_for_completion() {
    let directory = tempfile::tempdir().unwrap();
    let complete = directory.path().join("complete");
    let error = command::run_transaction(
        Path::new("/bin/sh"),
        &[
            "-c".into(),
            "/usr/bin/head -c 4194305 /dev/zero; /usr/bin/head -c 4194305 /dev/zero >&2; printf restored > \"$1\"".into(),
            "fixture".into(),
            complete.to_str().unwrap().into(),
        ],
        None,
        false,
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("exceeds 4 MiB"));
    assert_eq!(std::fs::read_to_string(complete).unwrap(), "restored");
}

#[test]
fn transaction_defers_input_failure_until_helper_finishes() {
    let directory = tempfile::tempdir().unwrap();
    let complete = directory.path().join("complete");
    let error = command::run_transaction(
        Path::new("/bin/sh"),
        &[
            "-c".into(),
            "exec 0<&-; /bin/sleep 0.1; printf restored > \"$1\"".into(),
            "fixture".into(),
            complete.to_str().unwrap().into(),
        ],
        Some(&vec![b'x'; 1024 * 1024]),
        false,
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("transaction input failed"));
    assert_eq!(std::fs::read_to_string(complete).unwrap(), "restored");
}

#[test]
fn transaction_streams_input_and_reports_helper_failure() {
    let payload = "λ".repeat(128 * 1024);
    let cancel = AtomicBool::new(false);
    let output = command::run_transaction(
        Path::new("/bin/cat"),
        &[],
        Some(payload.as_bytes()),
        false,
        &cancel,
    )
    .unwrap();
    assert_eq!(output, payload);
    let error = command::run_transaction(
        Path::new("/bin/sh"),
        &["-c".into(), "printf 'rollback failed' >&2; exit 7".into()],
        None,
        false,
        &cancel,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("rollback failed"));
}
