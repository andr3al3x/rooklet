//! Bounded, cancellable subprocess execution. Workers never prompt for credentials.
mod transaction;

use anyhow::{Context, Result, bail, ensure};
#[cfg(unix)]
use std::os::{fd::AsRawFd, unix::process::CommandExt};
use std::{
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const LIMIT: usize = 4 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);

pub fn run(
    path: &Path,
    args: &[String],
    input: Option<&[u8]>,
    privileged: bool,
    cancel: &AtomicBool,
) -> Result<String> {
    execute(path, args, input, privileged, cancel, false, TIMEOUT)
}
/// Includes successful stderr for tools whose result (such as a PF token) is written there.
pub fn run_combined(
    path: &Path,
    args: &[String],
    input: Option<&[u8]>,
    privileged: bool,
    cancel: &AtomicBool,
) -> Result<String> {
    execute(path, args, input, privileged, cancel, true, TIMEOUT)
}
/// A larger total budget for read-only helpers that perform several bounded tools.
pub fn run_with_timeout(
    path: &Path,
    args: &[String],
    input: Option<&[u8]>,
    privileged: bool,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<String> {
    ensure!(
        !timeout.is_zero() && timeout <= Duration::from_secs(90),
        "command timeout must be between zero and 90 seconds"
    );
    execute(path, args, input, privileged, cancel, false, timeout)
}
/// Wait for an authorized Rooklet transaction helper to finish, including rollback.
///
/// Only cancellation before launch is honored. The helper must bound its individual
/// tool calls and reap their children; an outer timeout could kill it during rollback.
/// Output remains bounded and is drained even after exceeding the capture limit.
pub fn run_transaction(
    path: &Path,
    args: &[String],
    input: Option<&[u8]>,
    privileged: bool,
    cancel: &AtomicBool,
) -> Result<String> {
    transaction::run(prepare(path, args, input, privileged, cancel)?, input)
        .with_context(|| format!("{} transaction helper", path.display()))
}

fn prepare(
    path: &Path,
    args: &[String],
    input: Option<&[u8]>,
    privileged: bool,
    cancel: &AtomicBool,
) -> Result<Command> {
    ensure!(
        path.is_absolute(),
        "command executable must be an absolute path"
    );
    ensure!(
        input.is_none_or(|data| data.len() <= LIMIT),
        "command input exceeds 4 MiB"
    );
    ensure!(!cancel.load(Ordering::Relaxed), "command cancelled");
    let mut command = if privileged && !is_root() {
        let mut command = Command::new("/usr/bin/sudo");
        command.args(["-n", "--"]).arg(path);
        command
    } else {
        Command::new(path)
    };
    command
        .args(args)
        .env("LC_ALL", "C")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate(&mut command);
    Ok(command)
}

fn execute(
    path: &Path,
    args: &[String],
    input: Option<&[u8]>,
    privileged: bool,
    cancel: &AtomicBool,
    combined: bool,
    timeout: Duration,
) -> Result<String> {
    let mut child = prepare(path, args, input, privileged, cancel)?
        .spawn()
        .with_context(|| format!("unable to start {}", path.display()))?;
    let result = (|| {
        let mut stdout = child.stdout.take().context("command stdout unavailable")?;
        let mut stderr = child.stderr.take().context("command stderr unavailable")?;
        nonblocking(&stdout)?;
        nonblocking(&stderr)?;
        let mut stdin = child.stdin.take();
        if let Some(pipe) = &stdin {
            nonblocking(pipe)?;
        }
        let mut sent = 0;
        let data = input.unwrap_or_default();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let deadline = Instant::now() + timeout;
        loop {
            ensure!(!cancel.load(Ordering::Relaxed), "command cancelled");
            ensure!(
                Instant::now() < deadline,
                "command timed out after {timeout:?}"
            );
            if let Some(pipe) = &mut stdin {
                match pipe.write(&data[sent..]) {
                    Ok(n) => sent += n,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error).context("command input failed"),
                }
                if sent == data.len() {
                    stdin.take();
                }
            }
            let done_out = drain(&mut stdout, &mut out)?;
            let done_err = drain(&mut stderr, &mut err)?;
            if let Some(status) = child.try_wait().context("command wait failed")?
                && done_out
                && done_err
            {
                if !status.success() {
                    let detail = String::from_utf8_lossy(&err);
                    bail!("{} failed ({}): {}", path.display(), status, detail.trim());
                }
                if combined {
                    ensure!(
                        out.len() + err.len() <= LIMIT,
                        "combined command output exceeds 4 MiB"
                    );
                    out.extend_from_slice(&err);
                }
                return String::from_utf8(out).context("command output is not UTF-8");
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    })();
    if result.is_err() {
        stop(&mut child);
    }
    result
}

pub fn is_root() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}
#[cfg(unix)]
pub(crate) fn nonblocking(pipe: &impl AsRawFd) -> Result<()> {
    let fd = pipe.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    ensure!(
        flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0,
        "unable to configure command pipe"
    );
    Ok(())
}
#[cfg(not(unix))]
pub(crate) fn nonblocking<T>(_: &T) -> Result<()> {
    bail!("macOS command pipes require Unix")
}

pub(crate) fn isolate(command: &mut Command) {
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}
pub(crate) fn stop(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}
fn drain(pipe: &mut impl Read, output: &mut Vec<u8>) -> Result<bool> {
    let mut buffer = [0; 8192];
    loop {
        match pipe.read(&mut buffer) {
            Ok(0) => return Ok(true),
            Ok(n) => {
                ensure!(output.len() + n <= LIMIT, "command output exceeds 4 MiB");
                output.extend_from_slice(&buffer[..n]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error).context("command output failed"),
        }
    }
}
