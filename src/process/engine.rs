//! Whole-request preflight and per-target identity checks, independent of the OS adapter.
use super::*;
use anyhow::{Context, ensure};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

pub(super) trait ProcessSystem {
    fn uid(&self) -> u32;
    fn own_pid(&self) -> u32;
    fn parent_pid(&mut self, pid: u32) -> Result<u32>;
    fn list_pids(&mut self) -> Result<Vec<u32>>;
    fn capture(&mut self, pid: u32) -> Result<ProcessIdentity>;
    fn signal(&mut self, identity: &ProcessIdentity, mode: TerminationMode) -> Result<()>;
}
pub(super) fn capture_snapshot(system: &mut impl ProcessSystem) -> Result<Vec<ProcessIdentity>> {
    let pids = system.list_pids()?;
    ensure!(
        pids.len() < MAX_PROCESSES,
        "process enumeration reached its safety limit"
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut snapshot = Vec::new();
    let mut seen = HashSet::new();
    for pid in pids {
        ensure!(Instant::now() < deadline, "process enumeration timed out");
        if pid <= 1 || !seen.insert(pid) {
            continue;
        }
        // Short-lived/inaccessible processes have no actionable identity in this snapshot.
        if let Ok(identity) = system.capture(pid)
            && identity.uid != 0
            && identity.uid == system.uid()
        {
            snapshot.push(identity);
        }
    }
    snapshot.sort_by_key(|identity| identity.pid);
    Ok(snapshot)
}
fn protected_pids(system: &mut impl ProcessSystem) -> Result<HashSet<u32>> {
    let mut protected = HashSet::from([0, 1]);
    let mut pid = system.own_pid();
    for _ in 0..128 {
        if pid <= 1 {
            return Ok(protected);
        }
        ensure!(
            protected.insert(pid),
            "cannot verify process ancestry: cycle detected"
        );
        pid = system
            .parent_pid(pid)
            .context("cannot verify protected ancestor processes")?;
    }
    anyhow::bail!("cannot verify process ancestry: chain exceeds safety limit")
}
fn check_target(target: &ProcessIdentity, uid: u32, protected: &HashSet<u32>) -> Result<()> {
    ensure!(
        target.pid > 1 && target.pid <= i32::MAX as u32 && !protected.contains(&target.pid),
        "PID {} is a protected process",
        target.pid
    );
    ensure!(
        target.uid != 0 && target.uid == uid,
        "PID {} is not an eligible current-user process",
        target.pid
    );
    ensure!(
        target.start_sec != 0
            && target.start_usec < 1_000_000
            && target.path.len() <= 4096
            && Path::new(&target.path).is_absolute(),
        "PID {} has an invalid captured identity",
        target.pid
    );
    Ok(())
}
fn check_current(system: &mut impl ProcessSystem, target: &ProcessIdentity) -> Result<()> {
    let current = system.capture(target.pid).with_context(|| {
        format!(
            "PID {} is unavailable; refresh before trying again",
            target.pid
        )
    })?;
    // Parent changes are normal after a helper is reparented. They do not change
    // process identity; ownership, start time, kernel generation and path do.
    ensure!(
        current.pid == target.pid
            && current.uid == target.uid
            && current.start_sec == target.start_sec
            && current.start_usec == target.start_usec
            && current.pid_version == target.pid_version
            && current.path == target.path
            && current.bundle_path == target.bundle_path,
        "PID {} changed identity; refresh before trying again",
        target.pid
    );
    Ok(())
}
pub(super) fn terminate(
    system: &mut impl ProcessSystem,
    request: &TerminationRequest,
) -> Result<TerminationReport> {
    ensure!(
        !request.targets.is_empty() && request.targets.len() <= MAX_TARGETS,
        "select between 1 and {MAX_TARGETS} captured processes"
    );
    let protected = protected_pids(system)?;
    let uid = system.uid();
    ensure!(
        uid != 0,
        "process termination is unavailable while Xield runs as root"
    );
    let mut seen = HashSet::new();
    for target in &request.targets {
        ensure!(seen.insert(target.pid), "duplicate process target");
        check_target(target, uid, &protected)?;
        check_current(system, target)?;
    }
    // No signal has been delivered before the entire request passes preflight.
    let mut report = TerminationReport {
        attempted: request.targets.len(),
        delivered: Vec::new(),
        failures: Vec::new(),
    };
    for target in &request.targets {
        let result =
            check_current(system, target).and_then(|()| system.signal(target, request.mode));
        match result {
            Ok(()) => report.delivered.push(target.pid),
            Err(error) => report.failures.push(SignalFailure {
                pid: target.pid,
                reason: error.to_string(),
            }),
        }
    }
    Ok(report)
}
