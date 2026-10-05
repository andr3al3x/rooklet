use rooklet::{
    backend::{parse_app_blocked, parse_applications, parse_settings, validate_application_path},
    command,
};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[test]
fn signed_flags_are_independent_and_unknown_state_is_rejected() {
    let settings = parse_settings("Firewall is enabled. (State = 1)\n", "Firewall stealth mode is off\n", "Firewall has block all state set to disabled.\n", "Automatically allow built-in signed software ENABLED.\nAutomatically allow downloaded signed software DISABLED.\n").unwrap();
    assert!(settings.enabled && settings.allow_signed);
    assert!(!settings.allow_signed_app && !settings.stealth && !settings.block_all);
    for signed in [
        "Automatically allow built-in signed software ENABLED.",
        "Automatically allow built-in signed software ENABLED.\nAutomatically allow built-in signed software DISABLED.",
        "unknown",
    ] {
        assert!(
            parse_settings(
                "Firewall is disabled. (State = 0)",
                "Firewall stealth mode is off",
                "Firewall has block all state set to disabled.",
                signed
            )
            .is_err()
        );
    }
    assert!(parse_settings("Firewall status unknown", "off", "disabled", "").is_err());
}
#[test]
fn app_list_preserves_paths_with_spaces_and_requires_matching_count() {
    let apps = parse_applications("Total number of apps = 2 \n1 : /Applications/Firefox.app\n             ( Allow incoming connections )\n2 : /Applications/Example App.app\n             ( Block incoming connections )\n").unwrap();
    assert_eq!(apps[1].name, "Example App");
    assert!(apps[1].blocked);
    assert!(!apps[0].blocked);
    assert_eq!(apps[1].path, "/Applications/Example App.app");
    for output in [
        "Total number of apps = 1",
        "Total number of apps = 0\n1 : /App.app\n( Allow incoming connections )",
        "Total number of apps = 1\n1 : /App.app\n( unknown )",
        "Total number of apps = 1\n2 : /App.app\n( Allow incoming connections )",
        "Total number of apps = 1\n1 : relative\n( Allow incoming connections )",
    ] {
        assert!(parse_applications(output).is_err(), "{output}");
    }
}
#[test]
fn app_verification_matches_the_requested_path() {
    assert!(
        !parse_app_blocked(
            "Incoming connection to /Applications/Example App.app is permitted.\n",
            "/Applications/Example App.app"
        )
        .unwrap()
    );
    assert!(
        parse_app_blocked(
            "Incoming connection to /Applications/Example App.app is blocked.",
            "/Applications/Example App.app"
        )
        .unwrap()
    );
    assert!(
        parse_app_blocked(
            "Incoming connection to /other.app is blocked.",
            "/Applications/Example App.app"
        )
        .is_err()
    );
    for path in [
        "relative.app",
        "/",
        "/Applications/../App.app",
        "/Applications/Bad\nName.app",
        "",
    ] {
        assert!(validate_application_path(path, false).is_err());
    }
    assert!(validate_application_path("/Applications/Example App.app", false).is_ok());
}
#[cfg(target_os = "macos")]
#[test]
fn cancelled_backend_rejects_work() {
    use rooklet::{
        backend::Backend,
        model::{Mutation, Setting},
    };
    let mut backend = Backend::new()
        .unwrap()
        .with_cancellation(Arc::new(AtomicBool::new(true)));
    assert!(backend.snapshot().is_err());
    assert!(backend.update_geoip().is_err());
    assert!(
        backend
            .mutate(Mutation::Setting(Setting::Firewall, false))
            .is_err()
    );
}
#[test]
fn runner_streams_stdin_stdout_and_reports_failure() {
    let cancel = AtomicBool::new(false);
    let payload = "λ".repeat(128 * 1024);
    assert_eq!(
        command::run(
            Path::new("/bin/cat"),
            &[],
            Some(payload.as_bytes()),
            false,
            &cancel
        )
        .unwrap(),
        payload
    );
    let error = command::run(
        Path::new("/bin/sh"),
        &["-c".into(), "printf 'expected failure' >&2; exit 7".into()],
        None,
        false,
        &cancel,
    )
    .unwrap_err();
    assert!(error.to_string().contains("expected failure"));
    let combined = command::run_combined(
        Path::new("/bin/sh"),
        &["-c".into(), "printf 'stdout'; printf 'stderr' >&2".into()],
        None,
        false,
        &cancel,
    )
    .unwrap();
    assert_eq!(combined, "stdoutstderr");
    assert!(command::run(Path::new("relative"), &[], None, false, &cancel).is_err());
    assert!(
        command::run(
            Path::new("/bin/cat"),
            &[],
            Some(&vec![0; 4 * 1024 * 1024 + 1]),
            false,
            &cancel
        )
        .is_err()
    );
}
#[test]
fn runner_output_bounds_and_cancellation_kill_descendants() {
    let cancel = Arc::new(AtomicBool::new(false));
    let error = command::run(
        Path::new("/usr/bin/head"),
        &["-c".into(), "4194305".into(), "/dev/zero".into()],
        None,
        false,
        &cancel,
    )
    .unwrap_err();
    assert!(error.to_string().contains("exceeds 4 MiB"));
    let signal = Arc::clone(&cancel);
    let thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        signal.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    let error = command::run(
        Path::new("/bin/sh"),
        &["-c".into(), "sleep 20 & wait".into()],
        None,
        false,
        &cancel,
    )
    .unwrap_err();
    assert!(error.to_string().contains("cancelled"));
    assert!(started.elapsed() < Duration::from_secs(2));
    thread.join().unwrap();
}

#[test]
fn runner_custom_timeout_is_bounded() {
    let started = Instant::now();
    let error = command::run_with_timeout(
        Path::new("/bin/sh"),
        &["-c".into(), "sleep 20 & wait".into()],
        None,
        false,
        &AtomicBool::new(false),
        Duration::from_millis(60),
    )
    .unwrap_err();
    assert!(error.to_string().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn actual_macos_app_list_has_compact_parentheses() {
    let output = "Total number of apps = 7 \n1 : /usr/libexec/remoted \n             (Allow incoming connections)\n2 : /usr/bin/python3 \n             (Allow incoming connections)\n3 : /usr/bin/ruby \n             (Allow incoming connections)\n4 : /usr/sbin/cupsd \n             (Allow incoming connections)\n5 : /usr/libexec/sharingd \n             (Allow incoming connections)\n6 : /usr/libexec/sshd-keygen-wrapper \n             (Allow incoming connections)\n7 : /usr/sbin/smbd \n             (Allow incoming connections)\n";
    let apps = parse_applications(output).unwrap();
    assert_eq!(apps.len(), 7);
    assert!(apps.iter().all(|app| !app.blocked));
    assert_eq!(apps[1].path, "/usr/bin/python3");
    assert!(
        parse_applications(
            "Total number of apps = 1\n1 : /App.app\n(Block incoming connections)\n"
        )
        .unwrap()[0]
            .blocked
    );
    assert!(
        parse_applications("Total number of apps = 1\n1 : /App.app\n(Allow unknown connections)\n")
            .is_err()
    );
}
