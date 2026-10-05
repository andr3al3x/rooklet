//! Own an isolated process group until cleanup, then reap its leader exactly once.
use std::{
    io,
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus},
};

pub(crate) struct ManagedChild {
    child: Option<Child>,
    status: Option<ExitStatus>,
}

impl ManagedChild {
    pub(crate) fn spawn(command: &mut Command) -> io::Result<Self> {
        super::isolate(command);
        Ok(Self {
            child: Some(command.spawn()?),
            status: None,
        })
    }

    pub(crate) fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.as_mut()?.stdout.take()
    }

    pub(crate) fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.as_mut()?.stderr.take()
    }

    pub(crate) fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.as_mut()?.stdin.take()
    }

    /// Observe exit without releasing the PID that reserves our process group ID.
    /// Only this owner may reap the child; the application installs no auto-reaper.
    pub(crate) fn exited(&mut self) -> io::Result<bool> {
        if self.status.is_some() {
            return Ok(true);
        }
        let child = self.child.as_mut().ok_or_else(no_child)?;
        #[cfg(unix)]
        {
            let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
            loop {
                // SAFETY: this exclusively owned child has not been reaped. The
                // aligned siginfo buffer is writable; WNOWAIT retains its PID.
                let result = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        child.id(),
                        info.as_mut_ptr(),
                        libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                    )
                };
                if result != -1 {
                    break;
                }
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                if error.raw_os_error() == Some(libc::ECHILD) {
                    // Fail closed if an external reaper violated our ownership.
                    self.child.take();
                }
                return Err(error);
            }
            // SAFETY: zero initialization is valid for siginfo_t's C fields, and
            // successful waitid supplies child information when an exit is pending.
            let info = unsafe { info.assume_init() };
            if info.si_signo == 0 {
                return Ok(false);
            }
            // SAFETY: WEXITED selects the SIGCHLD layout containing the child's PID.
            Ok(unsafe { info.si_pid() } != 0)
        }
        #[cfg(not(unix))]
        {
            self.status = child.try_wait()?;
            Ok(self.status.is_some())
        }
    }

    /// Clean the group before reaping, including descendants of an exited leader.
    pub(crate) fn finish(&mut self) -> io::Result<ExitStatus> {
        let result = self.finish_inner();
        if let Err(error) = &result {
            tracing::warn!(
                errno = error.raw_os_error(),
                outcome = "failed",
                "managed child cleanup failed"
            );
        }
        result
    }

    fn finish_inner(&mut self) -> io::Result<ExitStatus> {
        if let Some(status) = self.status {
            return Ok(status);
        }
        // Detect lost ownership before issuing any signal. This does not reap.
        let exited = self.exited()?;
        let child = self.child.as_mut().ok_or_else(no_child)?;
        #[cfg(unix)]
        let group_error = {
            let pid = i32::try_from(child.id())
                .ok()
                .filter(|pid| *pid > 1)
                .ok_or_else(|| io::Error::other("invalid child process group"))?;
            // SAFETY: spawn creates group PID, and exclusive non-reaping observation
            // keeps that PID reserved until wait below. No stale/reused group is
            // signaled. kill accepts this scalar group ID and a valid signal.
            let result = unsafe { libc::kill(-pid, libc::SIGKILL) };
            let error = (result == -1).then(io::Error::last_os_error);
            error.filter(|error| {
                error.raw_os_error() != Some(libc::ESRCH)
                    && !(exited
                        && error.raw_os_error() == Some(libc::EPERM)
                        && only_zombie_leader(pid))
            })
        };
        let killed = child.kill();
        if let Err(error) = killed
            && !self.exited()?
        {
            // A failed signal against a still-running child must not turn a
            // timeout into an unbounded wait. Retain ownership for later cleanup.
            return Err(error);
        }
        let status = match self.child.as_mut().ok_or_else(no_child)?.wait() {
            Ok(status) => status,
            Err(error) => {
                if error.raw_os_error() == Some(libc::ECHILD) {
                    self.child.take();
                }
                return Err(error);
            }
        };
        self.child.take();
        self.status = Some(status);
        #[cfg(unix)]
        if let Some(error) = group_error {
            return Err(error);
        }
        Ok(status)
    }

    pub(crate) fn stop(&mut self) {
        let _ = self.finish();
    }
}

/// macOS excludes zombies from group signals and returns EPERM for an empty
/// eligible group. Do not confuse that case with inaccessible live descendants.
#[cfg(target_os = "macos")]
fn only_zombie_leader(pid: i32) -> bool {
    const PROC_PGRP_ONLY: u32 = 2;
    let mut members = [0i32; 2];
    // SAFETY: PROC_PGRP_ONLY writes integer PIDs to this aligned, initialized
    // buffer, bounded by its byte size. The caller retains the exited leader's
    // unreaped PID. A second member or an incomplete query fails conservatively.
    let bytes = unsafe {
        libc::proc_listpids(
            PROC_PGRP_ONLY,
            pid as u32,
            members.as_mut_ptr().cast(),
            std::mem::size_of_val(&members) as i32,
        )
    };
    bytes == std::mem::size_of::<i32>() as i32 && members[0] == pid
}

#[cfg(all(unix, not(target_os = "macos")))]
fn only_zombie_leader(_: i32) -> bool {
    false
}

fn no_child() -> io::Error {
    io::Error::other("child process ownership is unavailable")
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        if self.child.is_some() {
            tracing::debug!(phase = "drop", "managed child fallback cleanup");
            self.stop();
        }
    }
}

#[cfg(all(test, unix))]
#[path = "child_tests.rs"]
mod tests;
