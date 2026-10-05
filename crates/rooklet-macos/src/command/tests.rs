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

#[derive(Clone, Default)]
struct LogCapture(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for LogCapture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) fn capture_logs(operation: impl FnOnce()) -> String {
    // With one registered subscriber, tracing-core uses the registering thread's
    // default dispatcher to cache interest. Retain an uninstalled registry so
    // unsubscribed parallel tests cannot cache `never` for a captured callsite.
    static INTEREST_REGISTRY: std::sync::OnceLock<tracing::Dispatch> = std::sync::OnceLock::new();
    INTEREST_REGISTRY.get_or_init(|| tracing::Dispatch::new(tracing_subscriber::registry()));
    let captured = LogCapture::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .without_time()
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::NEW)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, operation);
    String::from_utf8(captured.0.lock().unwrap().clone()).unwrap()
}

#[test]
fn command_logs_outcomes_without_arguments_paths_or_captured_output() {
    let logs = capture_logs(|| {
        let cancel = AtomicBool::new(false);
        let output = command::run(
            Path::new("/bin/sh"),
            &[
                "-c".into(),
                "printf 'PRIVATE_STDOUT_5ace'; printf 'PRIVATE_STDERR_fcb9' >&2".into(),
                "PRIVATE_ARGUMENT_cc85".into(),
            ],
            Some(b"PRIVATE_INPUT_3ee9"),
            false,
            &cancel,
        )
        .unwrap();
        assert_eq!(output, "PRIVATE_STDOUT_5ace");
        let error = command::run(
            Path::new("/bin/sh"),
            &[
                "-c".into(),
                "printf 'PRIVATE_FAILED_STDERR_892a' >&2; exit 7".into(),
            ],
            None,
            false,
            &cancel,
        )
        .unwrap_err();
        assert!(error.to_string().contains("PRIVATE_FAILED_STDERR_892a"));
        assert!(
            command::run(
                Path::new("/PRIVATE_EXECUTABLE_aae5/not-a-command"),
                &[],
                None,
                false,
                &cancel,
            )
            .is_err()
        );
    });
    assert!(logs.contains("command supervision completed"));
    assert!(logs.contains("success=true"));
    assert!(logs.contains("success=false"));
    assert!(logs.contains("exit_code=7"));
    assert!(logs.contains("duration_ms="));
    assert!(logs.contains("tool=\"other\""));
    assert!(
        !logs.contains("PRIVATE_"),
        "private fixture data leaked: {logs}"
    );
    assert!(!logs.contains("/bin/sh"));
}

#[test]
fn command_logs_timeout_and_cancellation_without_raw_errors() {
    let logs = capture_logs(|| {
        let error = command::run_with_timeout(
            Path::new("/bin/sh"),
            &["-c".into(), "sleep 5".into()],
            None,
            false,
            &AtomicBool::new(false),
            Duration::from_millis(30),
        )
        .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(
            command::run(
                Path::new("/bin/sh"),
                &["-c".into(), "PRIVATE_NEVER_RUN_d6dd".into()],
                None,
                false,
                &AtomicBool::new(true),
            )
            .is_err()
        );
    });
    assert!(logs.contains("WARN"));
    assert!(logs.contains("outcome=\"timeout\""));
    assert!(logs.contains("outcome=\"cancelled_before_launch\""));
    assert!(!logs.contains("PRIVATE_"));
    assert!(!logs.contains("command timed out after"));
}

#[test]
fn transaction_logs_completion_and_exit_status_without_stderr_or_input() {
    let logs = capture_logs(|| {
        let cancel = AtomicBool::new(false);
        let output = command::run_transaction(
            Path::new("/bin/cat"),
            &[],
            Some(b"PRIVATE_TRANSACTION_INPUT_d17b"),
            false,
            &cancel,
        )
        .unwrap();
        assert_eq!(output, "PRIVATE_TRANSACTION_INPUT_d17b");
        let error = command::run_transaction(
            Path::new("/bin/sh"),
            &[
                "-c".into(),
                "printf 'PRIVATE_TRANSACTION_STDERR_e65d' >&2; exit 9".into(),
            ],
            None,
            false,
            &cancel,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("PRIVATE_TRANSACTION_STDERR_e65d"));
    });
    assert!(logs.contains("transaction helper supervision completed"));
    assert!(logs.contains("exit_code=9"));
    assert!(logs.contains("success=true"));
    assert!(logs.contains("success=false"));
    assert!(
        !logs.contains("PRIVATE_"),
        "private fixture data leaked: {logs}"
    );
    assert!(!logs.contains("/bin/cat"));
    assert!(!logs.contains("/bin/sh"));
}

#[test]
fn scoped_capture_keeps_events_when_a_parallel_thread_registers_the_callsite() {
    fn emit_probe() {
        tracing::info!(outcome = "verified", "scoped capture callsite probe");
    }
    let logs = capture_logs(|| {
        // A thread-local subscriber does not follow work on another test thread.
        std::thread::spawn(emit_probe).join().unwrap();
        emit_probe();
    });
    assert!(
        logs.contains("outcome=\"verified\""),
        "captured logs: {logs}"
    );
    assert_eq!(logs.matches("scoped capture callsite probe").count(), 1);
}
