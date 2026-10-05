//! Long-lived nettop sampling over a nonblocking CSV terminal.
use super::{MAX_LINE, parser::CsvParser, tracker::Tracker};
use crate::{command, geoip::GeoIp, model::ProcessActivity};
use anyhow::{Context, Result, bail, ensure};
use std::{
    fs::File,
    io::Read,
    process::{Child, ChildStderr, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const MAX_READ: usize = 4 * 1024 * 1024;

#[cfg(unix)]
fn csv_terminal() -> Result<(File, File)> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let (mut master, mut slave) = (-1, -1);
    let status = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    ensure!(
        status == 0,
        "unable to create nettop CSV terminal: {}",
        std::io::Error::last_os_error()
    );
    let master = unsafe { File::from_raw_fd(master) };
    let slave = unsafe { File::from_raw_fd(slave) };
    for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        ensure!(
            flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } >= 0,
            "unable to configure nettop terminal descriptor"
        );
    }
    let mut settings = std::mem::MaybeUninit::<libc::termios>::uninit();
    ensure!(
        unsafe { libc::tcgetattr(slave.as_raw_fd(), settings.as_mut_ptr()) } == 0,
        "unable to read nettop terminal settings"
    );
    let mut settings = unsafe { settings.assume_init() };
    settings.c_oflag &= !libc::OPOST;
    ensure!(
        unsafe { libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &settings) } == 0,
        "unable to configure nettop CSV terminal"
    );
    command::nonblocking(&master)?;
    Ok((master, slave))
}
#[cfg(not(unix))]
fn csv_terminal() -> Result<(File, File)> {
    bail!("nettop CSV monitoring requires a Unix terminal")
}

pub struct Monitor {
    child: Child,
    stdout: File,
    stderr: ChildStderr,
    buffer: Vec<u8>,
    errors: Vec<u8>,
    parser: CsvParser,
    tracker: Tracker,
    latest: Vec<ProcessActivity>,
    started: Instant,
    last_sample: Option<Instant>,
    failed: Option<String>,
}
impl Monitor {
    pub fn start() -> Result<Self> {
        ensure!(
            cfg!(target_os = "macos"),
            "network activity monitoring requires macOS"
        );
        let mut command = Command::new("/usr/bin/nettop");
        command
            .args(["-L", "0", "-n", "-x", "-J", "bytes_in,bytes_out"])
            .env("LC_ALL", "C");
        Self::spawn(command)
    }
    fn spawn(mut command: Command) -> Result<Self> {
        // nettop block-buffers a pipe (16 KiB on macOS). Its documented -L logging
        // mode still emits CSV to a terminal, where stdio flushes each record.
        let (stdout, slave) = csv_terminal()?;
        command
            .stdin(Stdio::null())
            .stdout(Stdio::from(slave))
            .stderr(Stdio::piped());
        command::isolate(&mut command);
        let mut child = command.spawn().context("unable to start nettop")?;
        let setup = (|| {
            let stderr = child.stderr.take().context("nettop stderr unavailable")?;
            command::nonblocking(&stderr)?;
            Ok(stderr)
        })();
        let stderr = match setup {
            Ok(pipe) => pipe,
            Err(error) => {
                command::stop(&mut child);
                return Err(error);
            }
        };
        Ok(Self {
            child,
            stdout,
            stderr,
            buffer: Vec::new(),
            errors: Vec::new(),
            parser: CsvParser::default(),
            tracker: Tracker::default(),
            latest: Vec::new(),
            started: Instant::now(),
            last_sample: None,
            failed: None,
        })
    }
    pub fn has_sample(&self) -> bool {
        self.last_sample.is_some()
    }
    pub fn poll(&mut self, geoip: &mut GeoIp, cancel: &AtomicBool) -> Result<Vec<ProcessActivity>> {
        ensure!(
            !cancel.load(Ordering::Relaxed),
            "activity observation cancelled"
        );
        if let Some(error) = &self.failed {
            bail!("{error}");
        }
        let first_poll = self.last_sample.is_none();
        let deadline = Instant::now() + Duration::from_millis(1500);
        let result: Result<()> = (|| {
            loop {
                self.read_samples(geoip, cancel)?;
                if !first_poll || self.last_sample.is_some() || Instant::now() >= deadline {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        })();
        if let Err(error) = result {
            command::stop(&mut self.child);
            self.failed = Some(error.to_string());
            return Err(error);
        }
        let mut latest = self.latest.clone();
        if self
            .last_sample
            .is_some_and(|at| at.elapsed() > Duration::from_secs(3))
        {
            for process in &mut latest {
                process.rate_in = 0;
                process.rate_out = 0;
            }
        }
        Ok(latest)
    }
    fn read_samples(&mut self, geoip: &mut GeoIp, cancel: &AtomicBool) -> Result<()> {
        let mut chunk = [0; 8192];
        let mut read = 0;
        let mut closed = false;
        let mut samples = Vec::new();
        loop {
            ensure!(
                !cancel.load(Ordering::Relaxed),
                "activity observation cancelled"
            );
            match self.stdout.read(&mut chunk) {
                Ok(0) => {
                    closed = true;
                    break;
                }
                Ok(n) => {
                    read += n;
                    ensure!(
                        read <= MAX_READ,
                        "nettop output exceeds 4 MiB per observation"
                    );
                    self.buffer.extend_from_slice(&chunk[..n]);
                    while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
                        let line: Vec<_> = self.buffer.drain(..=end).collect();
                        if let Some(sample) = self.parser.push(&line)? {
                            samples.push(sample);
                        }
                    }
                    ensure!(
                        self.buffer.len() <= MAX_LINE,
                        "nettop CSV record exceeds 64 KiB"
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) if error.raw_os_error() == Some(libc::EIO) => {
                    closed = true;
                    break;
                }
                Err(error) => return Err(error).context("nettop read failed"),
            }
        }
        if !samples.is_empty() {
            let observed = Instant::now();
            self.latest =
                self.tracker
                    .update_batch(samples, observed.duration_since(self.started), geoip);
            self.last_sample = Some(observed);
        }
        loop {
            match self.stderr.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    ensure!(
                        self.errors.len() + n <= MAX_LINE,
                        "nettop diagnostics exceed 64 KiB"
                    );
                    self.errors.extend_from_slice(&chunk[..n]);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error).context("nettop diagnostic read failed"),
            }
        }
        if let Some(status) = self.child.try_wait()? {
            bail!(
                "nettop exited ({status}): {}",
                String::from_utf8_lossy(&self.errors).trim()
            );
        }
        ensure!(
            !closed,
            "nettop closed its CSV stream before producing another complete sample"
        );
        ensure!(
            self.last_sample
                .is_some_and(|at| at.elapsed() < Duration::from_secs(10))
                || self.started.elapsed() < Duration::from_secs(10),
            "nettop has not produced a sample for 10 seconds"
        );
        Ok(())
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        command::stop(&mut self.child);
    }
}

#[cfg(all(test, unix))]
mod terminal_tests {
    use super::*;
    use std::{
        os::fd::AsRawFd,
        sync::{Arc, atomic::AtomicBool},
    };
    #[test]
    fn csv_terminal_preserves_bytes_and_closes_descriptors_on_exec() {
        use std::io::Write;
        let (mut master, mut slave) = csv_terminal().unwrap();
        for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
            assert_ne!(
                unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC,
                0
            );
        }
        slave
            .write_all(b",bytes_in,bytes_out,\nExample.999999,1,2,\n")
            .unwrap();
        let mut bytes = [0; 128];
        let deadline = Instant::now() + Duration::from_secs(1);
        let count = loop {
            match master.read(&mut bytes) {
                Ok(count) => break count,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(1))
                }
                result => panic!("CSV terminal read failed: {result:?}"),
            }
        };
        assert_eq!(
            &bytes[..count],
            b",bytes_in,bytes_out,\nExample.999999,1,2,\n"
        );
    }
    #[test]
    fn first_poll_observes_logging_terminal_and_waits_for_complete_sample() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "test -t 1 || exit 3; printf ',bytes_in,bytes_out,\nExample.999999,100,50,\n'; sleep .1; printf ',bytes_in,bytes_out,\nExample.999999,150,70,\n'; sleep 20"]);
        let mut monitor = Monitor::spawn(command).unwrap();
        let mut geoip = GeoIp::default();
        let started = Instant::now();
        let samples = monitor.poll(&mut geoip, &AtomicBool::new(false)).unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(monitor.has_sample());
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].name, "Example");
        assert_eq!((samples[0].bytes_in, samples[0].rate_in), (0, 0));
    }
    #[test]
    fn queued_samples_keep_the_full_observation_rate() {
        let gate = tempfile::tempdir().unwrap();
        let ready = gate.path().join("ready");
        let written = gate.path().join("written");
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf ',bytes_in,bytes_out,\nExample.999999,100,50,\n,bytes_in,bytes_out,\n'; while ! test -f \"$1\"; do sleep .01; done; printf 'Example.999999,200,100,\n,bytes_in,bytes_out,\nExample.999999,300,150,\n,bytes_in,bytes_out,\n'; : >\"$2\"; sleep 20", "fixture"]);
        command.arg(&ready).arg(&written);
        let mut monitor = Monitor::spawn(command).unwrap();
        let mut geoip = GeoIp::default();
        let cancel = AtomicBool::new(false);
        let first = monitor.poll(&mut geoip, &cancel).unwrap();
        assert_eq!((first[0].bytes_in, first[0].rate_in), (0, 0));
        std::fs::write(ready, b"ready").unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !written.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(written.exists());
        // Simulate a delayed consumer without making the test wait for two seconds.
        monitor.started = Instant::now() - Duration::from_secs(2);
        let buffered = monitor.poll(&mut geoip, &cancel).unwrap();
        assert_eq!((buffered[0].bytes_in, buffered[0].bytes_out), (200, 100));
        assert!((95..=105).contains(&buffered[0].rate_in));
        assert!((45..=55).contains(&buffered[0].rate_out));
    }
    #[test]
    fn cancellation_interrupts_first_sample_wait_and_reaps_monitor() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 20"]);
        let mut monitor = Monitor::spawn(command).unwrap();
        let mut geoip = GeoIp::default();
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&cancel);
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            signal.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        assert!(
            monitor
                .poll(&mut geoip, &cancel)
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(monitor.child.try_wait().unwrap().is_some());
        thread.join().unwrap();
    }
    #[test]
    fn closed_terminal_reports_failure_instead_of_an_empty_activity_sample() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exit 0"]);
        let mut monitor = Monitor::spawn(command).unwrap();
        let mut geoip = GeoIp::default();
        assert!(monitor.poll(&mut geoip, &AtomicBool::new(false)).is_err());
    }
}
