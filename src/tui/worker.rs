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
    app::{ProfileOperation, ProfileOutcome},
    backend::Backend,
    model::{Mutation, Snapshot},
    process::{TerminationReport, TerminationRequest},
    profile,
};

pub(super) struct Update {
    pub result: Result<Snapshot>,
    pub operation_error: Option<anyhow::Error>,
    pub kind: UpdateKind,
    pub termination: Option<TerminationReport>,
    pub profile: Option<ProfileOutcome>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UpdateKind {
    Observation,
    Firewall,
    GeoIp,
    Terminate,
    Profile,
}

enum Work {
    Firewall(Mutation),
    GeoIp,
    Terminate(TerminationRequest),
    Profile(ProfileOperation),
}

impl Work {
    fn kind(&self) -> UpdateKind {
        match self {
            Self::Firewall(_) => UpdateKind::Firewall,
            Self::GeoIp => UpdateKind::GeoIp,
            Self::Terminate(_) => UpdateKind::Terminate,
            Self::Profile(_) => UpdateKind::Profile,
        }
    }

    fn run(self, backend: &mut Backend) -> Result<WorkReport> {
        let mut report = WorkReport::default();
        match self {
            Self::Firewall(mutation) => backend.mutate(mutation)?,
            Self::GeoIp => backend.update_geoip()?,
            Self::Terminate(request) => report.termination = Some(backend.terminate(&request)?),
            Self::Profile(operation) => report.profile = Some(profile_work(backend, operation)?),
        }
        Ok(report)
    }
}

#[derive(Default)]
struct WorkReport {
    termination: Option<TerminationReport>,
    profile: Option<ProfileOutcome>,
}

fn profile_work(backend: &mut Backend, operation: ProfileOperation) -> Result<ProfileOutcome> {
    match operation {
        ProfileOperation::List => Ok(ProfileOutcome::Listed(profile::list()?)),
        ProfileOperation::Prepare(name) => {
            let profile = profile::load(&name)?;
            let prepared = profile::prepare(&profile, &backend.snapshot()?)?;
            Ok(ProfileOutcome::Prepared {
                name,
                prepared: Box::new(prepared),
            })
        }
        ProfileOperation::Export(name) => {
            let profile = profile::export(&backend.snapshot()?)?;
            let mut entries = profile::list()?;
            profile::save(&name, &profile)?;
            entries.push(name.clone());
            entries.sort();
            Ok(ProfileOutcome::Exported { name, entries })
        }
        ProfileOperation::Apply(prepared) => {
            profile::apply(backend, &prepared)?;
            Ok(ProfileOutcome::Applied)
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
    pub fn start() -> Result<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let (commands, work) = mpsc::sync_channel(1);
        let (responses, updates) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("xield-backend".into())
            .spawn(move || {
                // Database validation and monitor startup must not block the first TUI frame.
                let mut backend = match Backend::new() {
                    Ok(backend) => backend.with_cancellation(Arc::clone(&worker_cancel)),
                    Err(error) => {
                        let _ = publish(
                            &responses,
                            Update {
                                result: Err(error),
                                operation_error: None,
                                kind: UpdateKind::Observation,
                                termination: None,
                                profile: None,
                            },
                        );
                        return;
                    }
                };
                observe(work, responses, &worker_cancel, |work| {
                    let kind = work.as_ref().map_or(UpdateKind::Observation, Work::kind);
                    let report = work
                        .map(|work| work.run(&mut backend))
                        .transpose()
                        .map(Option::unwrap_or_default);
                    observe_after_work(kind, report, || backend.snapshot())
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

    pub fn profile(&mut self, operation: ProfileOperation) -> Result<()> {
        self.submit_work(Work::Profile(operation))
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

fn observe_after_work(
    kind: UpdateKind,
    report: Result<WorkReport>,
    snapshot: impl FnOnce() -> Result<Snapshot>,
) -> Update {
    let (report, operation_error) = match report {
        Ok(report) => (report, None),
        Err(error) => (WorkReport::default(), Some(error)),
    };
    // Failed operations may have changed state. Read back even after a partial
    // failure, and keep the action error separate from observation availability.
    Update {
        result: snapshot(),
        operation_error,
        kind,
        termination: report.termination,
        profile: report.profile,
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
