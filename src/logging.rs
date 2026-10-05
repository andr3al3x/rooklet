//! Optional, bounded diagnostic logs. Only explicitly instrumented fields are recorded.

mod directory;
mod sink;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, ValueEnum};
use tracing::Dispatch;
use tracing_subscriber::{
    filter::{LevelFilter, Targets},
    layer::SubscriberExt,
};

#[derive(Debug, Args)]
pub(crate) struct Options {
    /// Write private diagnostic JSONL files to this directory.
    #[arg(long, global = true, value_name = "DIRECTORY")]
    pub(crate) log_dir: Option<PathBuf>,
    /// Minimum diagnostic severity (requires --log-dir).
    #[arg(
        long,
        global = true,
        value_enum,
        default_value = "info",
        requires = "log_dir"
    )]
    pub(crate) log_level: Level,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub(crate) enum Level {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl Level {
    fn filter(self) -> LevelFilter {
        match self {
            Self::Error => LevelFilter::ERROR,
            Self::Warn => LevelFilter::WARN,
            Self::Info => LevelFilter::INFO,
            Self::Debug => LevelFilter::DEBUG,
            Self::Trace => LevelFilter::TRACE,
        }
    }
}

impl Options {
    pub(crate) fn start(&self) -> Result<Session> {
        let Some(path) = &self.log_dir else {
            return Ok(Session { worker: None });
        };
        let (session, dispatch) = start(path, self.log_level, sink::Limits::default())?;
        tracing::dispatcher::set_global_default(dispatch)
            .context("could not initialize diagnostic logging")?;
        Ok(session)
    }
}

/// Keep this alive until all application workers have stopped.
pub(crate) struct Session {
    worker: Option<sink::Worker>,
}

impl Session {
    /// Give the sink up to one second to drain accepted records and write counters.
    /// Stalled filesystem I/O is best effort; the sink alone owns the final write.
    pub(crate) fn finish(mut self) {
        if let Some(worker) = self.worker.take() {
            worker.finish();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.finish();
        }
    }
}

fn start(
    path: &std::path::Path,
    level: Level,
    limits: sink::Limits,
) -> Result<(Session, Dispatch)> {
    let file = directory::create_run(path, limits.retained_files)?;
    let (writer, worker) = sink::spawn(file, limits)?;
    let targets = Targets::new()
        .with_target("rooklet", level.filter())
        .with_target("rooklet_macos", level.filter())
        .with_default(LevelFilter::OFF);
    let subscriber = tracing_subscriber::registry().with(targets).with(
        tracing_subscriber::fmt::layer()
            .json()
            .with_ansi(false)
            .with_current_span(true)
            .with_span_list(true)
            .event_format(writer.event_formatter())
            .log_internal_errors(false)
            .with_writer(writer),
    );
    Ok((
        Session {
            worker: Some(worker),
        },
        Dispatch::new(subscriber),
    ))
}

#[cfg(test)]
mod tests;
