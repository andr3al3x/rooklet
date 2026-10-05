use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields, format};
use tracing_subscriber::registry::LookupSpan;

// Leave space for counters even when the event budget has been exhausted.
const SUMMARY_RESERVE: usize = 1024;

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub(super) queue_records: usize,
    pub(super) record_bytes: usize,
    pub(super) file_bytes: usize,
    pub(super) retained_files: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            queue_records: 512,
            record_bytes: 16 * 1024,
            file_bytes: 4 * 1024 * 1024,
            retained_files: 5,
        }
    }
}

#[derive(Default)]
struct Counters {
    queue_dropped: AtomicU64,
    oversized_records: AtomicU64,
    file_budget_dropped: AtomicU64,
    write_errors: AtomicU64,
}

#[derive(Clone)]
pub(super) struct Writer {
    sender: mpsc::SyncSender<Vec<u8>>,
    counters: Arc<Counters>,
    record_bytes: usize,
}

impl Writer {
    pub(super) fn event_formatter(&self) -> EventFormatter {
        EventFormatter {
            counters: Arc::clone(&self.counters),
            record_bytes: self.record_bytes,
        }
    }
}

pub(super) struct EventFormatter {
    counters: Arc<Counters>,
    record_bytes: usize,
}

struct BoundedFormatWriter<'a> {
    inner: format::Writer<'a>,
    remaining: usize,
    exceeded: bool,
}

impl fmt::Write for BoundedFormatWriter<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if value.len() > self.remaining {
            self.exceeded = true;
            return Err(fmt::Error);
        }
        self.remaining -= value.len();
        self.inner.write_str(value)
    }
}

impl<S, N> FormatEvent<S, N> for EventFormatter
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        writer: format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> fmt::Result {
        // Bound the formatter's thread-local buffer before MakeWriter sees it.
        let mut writer = BoundedFormatWriter {
            inner: writer,
            remaining: self.record_bytes,
            exceeded: false,
        };
        let result = format::format()
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .format_event(ctx, format::Writer::new(&mut writer), event);
        if writer.exceeded {
            self.counters
                .oversized_records
                .fetch_add(1, Ordering::Relaxed);
            return Err(fmt::Error);
        }
        result
    }
}

pub(super) struct RecordWriter {
    writer: Writer,
    bytes: Vec<u8>,
    oversized: bool,
}

impl<'a> MakeWriter<'a> for Writer {
    type Writer = RecordWriter;

    fn make_writer(&'a self) -> Self::Writer {
        RecordWriter {
            writer: self.clone(),
            bytes: Vec::new(),
            oversized: false,
        }
    }
}

impl Write for RecordWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.oversized {
            if bytes.len() > self.writer.record_bytes.saturating_sub(self.bytes.len()) {
                self.oversized = true;
                self.bytes.clear();
            } else {
                self.bytes.extend_from_slice(bytes);
            }
        }
        // Never surface sink errors through tracing's formatter or terminal output.
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for RecordWriter {
    fn drop(&mut self) {
        if self.oversized {
            self.writer
                .counters
                .oversized_records
                .fetch_add(1, Ordering::Relaxed);
        } else if !self.bytes.is_empty()
            && self
                .writer
                .sender
                .try_send(std::mem::take(&mut self.bytes))
                .is_err()
        {
            self.writer
                .counters
                .queue_dropped
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub(super) struct Worker {
    shutdown: Arc<AtomicBool>,
    completed: mpsc::Receiver<()>,
}

impl Worker {
    pub(super) fn finish(self) {
        // Operation threads never wait for the sink. A stalled filesystem must
        // not hold shutdown open indefinitely or print a terminal diagnostic.
        self.shutdown.store(true, Ordering::Release);
        let _ = self.completed.recv_timeout(Duration::from_secs(1));
    }
}

pub(super) fn spawn(file: File, limits: Limits) -> Result<(Writer, Worker)> {
    let (sender, receiver) = mpsc::sync_channel(limits.queue_records);
    let counters = Arc::new(Counters::default());
    let sink_counters = Arc::clone(&counters);
    let shutdown = Arc::new(AtomicBool::new(false));
    let sink_shutdown = Arc::clone(&shutdown);
    let (completed_sender, completed) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("rooklet-log".into())
        .spawn(move || {
            drain(file, receiver, &sink_counters, &sink_shutdown, limits);
            let _ = completed_sender.try_send(());
        })
        .context("could not start diagnostic log writer")?;
    Ok((
        Writer {
            sender: sender.clone(),
            counters,
            record_bytes: limits.record_bytes,
        },
        Worker {
            shutdown,
            completed,
        },
    ))
}

fn drain(
    mut file: File,
    receiver: mpsc::Receiver<Vec<u8>>,
    counters: &Counters,
    shutdown: &AtomicBool,
    limits: Limits,
) {
    let mut written = 0usize;
    let mut failed = false;
    let budget = limits.file_bytes.saturating_sub(SUMMARY_RESERVE);
    loop {
        let bytes = if shutdown.load(Ordering::Acquire) {
            match receiver.try_recv() {
                Ok(bytes) => bytes,
                Err(_) => break,
            }
        } else {
            match receiver.recv_timeout(Duration::from_millis(25)) {
                Ok(bytes) => bytes,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        };
        if failed {
            continue;
        } else if bytes.len() > budget.saturating_sub(written) {
            counters.file_budget_dropped.fetch_add(1, Ordering::Relaxed);
        } else {
            if file.write_all(&bytes).is_err() {
                counters.write_errors.fetch_add(1, Ordering::Relaxed);
                failed = true;
                // Preserve complete JSONL records when a write was partial.
                // Further writes, including the summary, are disabled.
                if file.set_len(written as u64).is_err() {
                    counters.write_errors.fetch_add(1, Ordering::Relaxed);
                }
            } else {
                written += bytes.len();
            }
        }
    }
    let queue_dropped = counters.queue_dropped.load(Ordering::Relaxed);
    let oversized_records = counters.oversized_records.load(Ordering::Relaxed);
    let file_budget_dropped = counters.file_budget_dropped.load(Ordering::Relaxed);
    let write_errors = counters.write_errors.load(Ordering::Relaxed);
    let loss =
        queue_dropped > 0 || oversized_records > 0 || file_budget_dropped > 0 || write_errors > 0;
    let summary = serde_json::json!({
        "level": if loss { "WARN" } else { "INFO" },
        "target": "rooklet::logging",
        "fields": {
            "event": "logging_finished",
            "queue_dropped": queue_dropped,
            "oversized_records": oversized_records,
            "file_budget_dropped": file_budget_dropped,
            "write_errors": write_errors,
        }
    });
    if !failed && let Ok(mut bytes) = serde_json::to_vec(&summary) {
        bytes.push(b'\n');
        if bytes.len() <= limits.file_bytes.saturating_sub(written)
            && file.write_all(&bytes).is_err()
        {
            counters.write_errors.fetch_add(1, Ordering::Relaxed);
            // A failed summary must not leave a partial JSON record either.
            if file.set_len(written as u64).is_err() {
                counters.write_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    if file.flush().is_err() {
        counters.write_errors.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_queue_discards_whole_records_without_waiting() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let counters = Arc::new(Counters::default());
        let writer = Writer {
            sender,
            counters: Arc::clone(&counters),
            record_bytes: 512,
        };
        for record in [b"first\n", b"later\n"] {
            let mut record_writer = writer.make_writer();
            record_writer.write_all(record).unwrap();
        }
        assert_eq!(receiver.try_recv().unwrap(), b"first\n");
        assert!(receiver.try_recv().is_err());
        assert_eq!(counters.queue_dropped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn failed_file_writes_are_counted_and_stop_future_writes() {
        let temporary = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temporary.path(), b"unchanged\n").unwrap();
        let read_only = File::open(temporary.path()).unwrap();
        let (sender, receiver) = mpsc::sync_channel(2);
        sender.send(b"first\n".to_vec()).unwrap();
        sender.send(b"later\n".to_vec()).unwrap();
        let counters = Counters::default();
        drain(
            read_only,
            receiver,
            &counters,
            &AtomicBool::new(true),
            Limits::default(),
        );
        assert_eq!(counters.write_errors.load(Ordering::Relaxed), 2);
        assert_eq!(std::fs::read(temporary.path()).unwrap(), b"unchanged\n");
    }
}
