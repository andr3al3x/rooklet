//! Only processes created by these tests are observed or stopped.
use super::*;
use std::{
    process::Stdio,
    time::{Duration, Instant},
};

fn wait_for_exit(child: &mut ManagedChild) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !child.exited().unwrap() {
        assert!(Instant::now() < deadline, "fixture did not exit");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn exit_observation_retains_pid_until_cleanup_and_preserves_status() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "exit 7"]);
    let mut child = ManagedChild::spawn(&mut command).unwrap();
    let pid = child.child.as_ref().unwrap().id() as i32;
    wait_for_exit(&mut child);
    assert!(child.exited().unwrap());
    // SAFETY: signal zero only queries the test's own child. WNOWAIT must keep
    // its zombie/PID present until finish, rather than releasing its group ID.
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
    assert_eq!(child.finish().unwrap().code(), Some(7));
    assert!(child.child.is_none());
    // Cached status makes repeated cleanup independent of the old numeric PID.
    assert_eq!(child.finish().unwrap().code(), Some(7));
    child.stop();
    assert!(child.exited().unwrap());
}

#[test]
fn cleanup_stops_descendants_after_the_leader_exits() {
    let directory = tempfile::tempdir().unwrap();
    let leaked = directory.path().join("leaked");
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-c",
            "(/bin/sleep .4; printf leaked > \"$1\") & exit 7",
            "fixture",
        ])
        .arg(&leaked)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = ManagedChild::spawn(&mut command).unwrap();
    wait_for_exit(&mut child);
    assert_eq!(child.finish().unwrap().code(), Some(7));
    drop(child);
    std::thread::sleep(Duration::from_millis(500));
    assert!(!leaked.exists(), "descendant survived group cleanup");
}

#[test]
fn drop_stops_and_reaps_a_running_child() {
    let mut command = Command::new("/bin/sleep");
    command.arg("20");
    let child = ManagedChild::spawn(&mut command).unwrap();
    let pid = child.child.as_ref().unwrap().id() as i32;
    drop(child);
    assert_eq!(
        // SAFETY: waitpid accepts a null optional status pointer and only queries the
        // exact child created here. Drop must have already reaped it.
        unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) },
        -1
    );
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn unexpected_reaping_disarms_group_cleanup() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "exit 0"]);
    let mut child = ManagedChild::spawn(&mut command).unwrap();
    // Simulate an external reaper violating the owner's exclusive wait contract.
    child.child.as_mut().unwrap().wait().unwrap();
    assert_eq!(
        child.exited().unwrap_err().raw_os_error(),
        Some(libc::ECHILD)
    );
    assert!(child.child.is_none());
    assert!(child.finish().is_err());
    child.stop();
}
