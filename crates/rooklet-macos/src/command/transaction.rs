//! Supervision for helpers whose bounded operations may require rollback.
#[cfg(unix)]
mod unix {
    use super::super::{LIMIT, isolate, nonblocking};
    use anyhow::{Context, Result, bail, ensure};
    use std::{
        io::{PipeReader, Read, Write, pipe},
        process::{Command, Stdio},
        time::Duration,
    };

    struct Capture {
        pipe: Option<PipeReader>,
        bytes: Vec<u8>,
        overflow: bool,
        error: Option<std::io::Error>,
    }
    impl Capture {
        fn new(pipe: PipeReader) -> Self {
            Self {
                pipe: Some(pipe),
                bytes: Vec::new(),
                overflow: false,
                error: None,
            }
        }
        /// Drain a bounded amount per turn so both streams and stdin get serviced.
        /// True means no more bytes are currently available, including an open pipe.
        fn drain(&mut self) -> bool {
            let Some(pipe) = &mut self.pipe else {
                return true;
            };
            let mut buffer = [0; 8192];
            for _ in 0..8 {
                match pipe.read(&mut buffer) {
                    Ok(0) => {
                        self.pipe.take();
                        return true;
                    }
                    Ok(count) => {
                        let keep = count.min(LIMIT - self.bytes.len());
                        self.bytes.extend_from_slice(&buffer[..keep]);
                        self.overflow |= keep != count;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return true,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        self.error = Some(error);
                        self.pipe.take();
                        return true;
                    }
                }
            }
            false
        }
    }

    pub(super) fn run(mut command: Command, input: Option<&[u8]>) -> Result<String> {
        // Complete all descriptor setup before launching an authorized helper.
        let (out_read, out_write) = pipe()?;
        let (err_read, err_write) = pipe()?;
        nonblocking(&out_read)?;
        nonblocking(&err_read)?;
        command
            .stdout(Stdio::from(out_write))
            .stderr(Stdio::from(err_write));
        let mut stdin = if input.is_some() {
            let (read, write) = pipe()?;
            nonblocking(&write)?;
            command.stdin(Stdio::from(read));
            Some(write)
        } else {
            None
        };
        isolate(&mut command);
        let mut child = command
            .spawn()
            .context("unable to start transaction helper")?;
        // Command retains the configured descriptors; close those parent copies.
        drop(command);
        let mut out = Capture::new(out_read);
        let mut err = Capture::new(err_read);
        let mut sent = 0;
        let data = input.unwrap_or_default();
        let mut input_error = None;
        let mut status = None;
        loop {
            if let Some(pipe) = &mut stdin {
                match pipe.write(&data[sent..]) {
                    Ok(count) => sent += count,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => {
                        input_error = Some(error);
                        stdin.take();
                    }
                }
                if sent == data.len() {
                    stdin.take();
                }
            }
            let out_empty = out.drain();
            let err_empty = err.drain();
            if status.is_none() {
                status = match child.try_wait() {
                    Ok(Some(status)) => Some(Ok(status)),
                    Ok(None) => None,
                    Err(error) => Some(Err(error)),
                };
                // Drain once more after observing exit to capture the final writes.
            } else if out_empty && err_empty {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        // No cancellation, outer deadline, or I/O error kills the transaction.
        // Once it exits, inherited pipe handles cannot keep supervision waiting.
        let status = status.unwrap().context("transaction wait failed")?;
        tracing::debug!(
            exit_code = status.code(),
            success = status.success(),
            "transaction helper exited"
        );
        if !status.success() {
            let detail = String::from_utf8_lossy(&err.bytes);
            let truncated = if out.overflow || err.overflow {
                " (command output exceeds 4 MiB; truncated)"
            } else {
                ""
            };
            bail!("helper failed ({status}){truncated}: {}", detail.trim());
        }
        if input_error.is_some()
            || out.error.is_some()
            || err.error.is_some()
            || out.overflow
            || err.overflow
        {
            tracing::warn!(
                input_failed = input_error.is_some(),
                output_failed = out.error.is_some() || err.error.is_some(),
                output_overflow = out.overflow || err.overflow,
                "transaction helper completed with supervision failures"
            );
        }
        if let Some(error) = input_error {
            return Err(error).context("transaction input failed");
        }
        ensure!(
            sent == data.len(),
            "transaction input failed: helper exited before input was sent"
        );
        if let Some(error) = out.error.or(err.error) {
            return Err(error).context("transaction output failed");
        }
        ensure!(
            !out.overflow && !err.overflow,
            "command output exceeds 4 MiB; transaction helper completed"
        );
        String::from_utf8(out.bytes).context("command output is not UTF-8")
    }
}

pub(super) fn run(command: std::process::Command, input: Option<&[u8]>) -> anyhow::Result<String> {
    #[cfg(unix)]
    {
        unix::run(command, input)
    }
    #[cfg(not(unix))]
    {
        let _ = (command, input);
        anyhow::bail!("transaction helpers require Unix command pipes")
    }
}
