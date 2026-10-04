//! Bounded backend work and observations, with transaction-aware shutdown.
use anyhow::{Context, Result, ensure};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use xield::{
    backend::Backend,
    model::{Mutation, Snapshot},
    process::{TerminationReport, TerminationRequest},
};

pub(super) struct Update {
    pub result: Result<Snapshot>,
    pub kind: UpdateKind,
    pub termination: Option<TerminationReport>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UpdateKind {
    Observation,
    Firewall,
    GeoIp,
    Terminate,
}

enum Work {
    Firewall(Mutation),
    GeoIp,
    Terminate(TerminationRequest),
}

impl Work {
    fn kind(&self) -> UpdateKind {
        match self {
            Self::Firewall(_) => UpdateKind::Firewall,
            Self::GeoIp => UpdateKind::GeoIp,
            Self::Terminate(_) => UpdateKind::Terminate,
        }
    }

    fn run(self, backend: &mut Backend) -> Result<Option<TerminationReport>> {
        match self {
            Self::Firewall(mutation) => backend.mutate(mutation).map(|()| None),
            Self::GeoIp => backend.update_geoip().map(|()| None),
            Self::Terminate(request) => backend.terminate(&request).map(Some),
        }
    }
}

pub(super) struct Worker {
    commands: Option<SyncSender<Work>>,
    updates: Option<Receiver<Update>>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    in_flight: bool,
}

impl Worker {
    pub fn start(demo: bool) -> Result<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let (commands, work) = mpsc::sync_channel(1);
        let (responses, updates) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("xield-backend".into())
            .spawn(move || {
                // Database validation and monitor startup must not block the first TUI frame.
                let mut backend = match Backend::new(demo) {
                    Ok(backend) => backend.with_cancellation(Arc::clone(&worker_cancel)),
                    Err(error) => {
                        let _ = publish(
                            &responses,
                            Update {
                                result: Err(error),
                                kind: UpdateKind::Observation,
                                termination: None,
                            },
                        );
                        return;
                    }
                };
                observe(work, responses, &worker_cancel, |work| {
                    let kind = work.as_ref().map_or(UpdateKind::Observation, Work::kind);
                    let termination = match work.map(|work| work.run(&mut backend)).transpose() {
                        Ok(report) => report.flatten(),
                        Err(error) => {
                            return Update {
                                result: Err(error),
                                kind,
                                termination: None,
                            };
                        }
                    };
                    // Preserve delivered signals even if the subsequent observation fails.
                    Update {
                        result: backend.snapshot(),
                        kind,
                        termination,
                    }
                });
            })
            .context("cannot start backend worker")?;
        Ok(Self {
            commands: Some(commands),
            updates: Some(updates),
            cancel,
            thread: Some(thread),
            in_flight: false,
        })
    }

    pub fn submit(&mut self, mutation: Mutation) -> Result<()> {
        self.submit_work(Work::Firewall(mutation))
    }

    pub fn update_geoip(&mut self) -> Result<()> {
        self.submit_work(Work::GeoIp)
    }

    pub fn terminate(&mut self, request: TerminationRequest) -> Result<()> {
        self.submit_work(Work::Terminate(request))
    }

    fn submit_work(&mut self, work: Work) -> Result<()> {
        ensure!(!self.in_flight, "an operation is already in progress");
        self.commands
            .as_ref()
            .context("backend worker stopped")?
            .send(work)
            .context("backend worker stopped")?;
        self.in_flight = true;
        Ok(())
    }

    pub fn drain(&mut self) -> Vec<Update> {
        let updates: Vec<_> = self.updates.iter().flat_map(Receiver::try_iter).collect();
        if updates
            .iter()
            .any(|update| update.kind != UpdateKind::Observation)
        {
            self.in_flight = false;
        }
        updates
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // An accepted mutation must finish before its subprocess is cancelled.
        if !self.in_flight {
            self.cancel.store(true, Ordering::Relaxed);
        }
        self.commands.take();
        // Unblock a pending response before joining, including on terminal errors.
        self.updates.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn observe(
    work: Receiver<Work>,
    responses: SyncSender<Update>,
    cancel: &AtomicBool,
    mut snapshot: impl FnMut(Option<Work>) -> Update,
) {
    let mut next_poll = Instant::now();
    while !cancel.load(Ordering::Relaxed) {
        let wait = next_poll
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(100));
        let update = match work.recv_timeout(wait) {
            Ok(work) => {
                let kind = work.kind();
                let mut update = snapshot(Some(work));
                update.kind = kind;
                update
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() >= next_poll => {
                next_poll = Instant::now() + Duration::from_secs(1);
                snapshot(None)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
        };
        let change = update.kind != UpdateKind::Observation;
        // A disconnected display can coexist with an accepted, queued mutation.
        // Finish command work before exiting; a periodic observation cannot end it.
        if !publish(&responses, update) && change {
            break;
        }
    }
}

fn publish(responses: &SyncSender<Update>, update: Update) -> bool {
    if update.kind != UpdateKind::Observation {
        responses.send(update).is_ok()
    } else {
        matches!(
            responses.try_send(update),
            Ok(()) | Err(mpsc::TrySendError::Full(_))
        )
    }
}

#[cfg(test)]
mod tests;
