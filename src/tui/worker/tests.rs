use super::*;
use rooklet_core::{
    model::{FirewallSettings, Setting},
    process::TerminationMode,
};

fn snapshot(stealth: bool) -> Snapshot {
    Snapshot {
        firewall: Some(FirewallSettings {
            stealth,
            ..FirewallSettings::default()
        }),
        ..Snapshot::default()
    }
}

fn update(result: Result<Snapshot>) -> Update {
    Update {
        result,
        operation_error: None,
        kind: UpdateKind::Observation,
        termination: None,
        profile: None,
    }
}

fn worker_with(callback: impl FnMut(Option<Work>) -> Update + Send + 'static) -> Worker {
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let (commands, work) = mpsc::sync_channel(1);
    let (responses, updates) = mpsc::sync_channel(1);
    let thread = thread::spawn(move || observe(work, responses, &worker_cancel, callback));
    Worker {
        commands: Some(commands),
        updates: Some(updates),
        cancel,
        thread: Some(thread),
        in_flight: false,
        interest: Arc::new(Mutex::new(ResourceInterest::default())),
        refresh: Arc::new(AtomicBool::new(false)),
    }
}

fn wait_for(worker: &mut Worker, kind: UpdateKind) -> Update {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(update) = worker
            .drain()
            .into_iter()
            .find(|update| update.kind == kind)
        {
            return update;
        }
        assert!(Instant::now() < deadline, "operation result was lost");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn queued_observation_does_not_drop_mutation_result() {
    let (responses, updates) = mpsc::sync_channel(1);
    assert!(publish(&responses, update(Ok(snapshot(false)))));
    // Another observation is skipped while the bounded queue is full.
    assert!(publish(&responses, update(Ok(snapshot(false)))));
    let thread = thread::spawn(move || {
        publish(
            &responses,
            Update {
                kind: UpdateKind::Firewall,
                ..update(Ok(snapshot(true)))
            },
        )
    });
    assert_eq!(
        updates.recv_timeout(Duration::from_secs(3)).unwrap().kind,
        UpdateKind::Observation
    );
    let update = updates.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(update.kind, UpdateKind::Firewall);
    assert!(update.result.unwrap().firewall.unwrap().stealth);
    assert!(thread.join().unwrap());
}

#[test]
fn shutdown_with_pending_work_and_undrained_responses_does_not_deadlock() {
    let completed = Arc::new(AtomicBool::new(false));
    let worker_completed = Arc::clone(&completed);
    let (finished, completion) = mpsc::channel();
    let thread = thread::spawn(move || {
        let mut worker = worker_with(move |work| {
            if let Some(Work::Firewall(Mutation::Setting(Setting::Stealth, true))) = work {
                worker_completed.store(true, Ordering::Relaxed);
            }
            update(Ok(snapshot(true)))
        });
        worker
            .submit(Mutation::Setting(Setting::Stealth, true))
            .unwrap();
        drop(worker);
        finished.send(()).unwrap();
    });
    completion.recv_timeout(Duration::from_secs(3)).unwrap();
    thread.join().unwrap();
    assert!(completed.load(Ordering::Relaxed));
}

#[test]
fn shutdown_during_observation_finishes_the_accepted_mutation() {
    let completed = Arc::new(AtomicBool::new(false));
    let worker_completed = Arc::clone(&completed);
    let (started, observing) = mpsc::channel();
    let (release, paused) = mpsc::channel();
    let mut worker = worker_with(move |work| {
        if let Some(work) = work {
            assert!(matches!(
                work,
                Work::Firewall(Mutation::Setting(Setting::Stealth, true))
            ));
            worker_completed.store(true, Ordering::Relaxed);
        } else {
            started.send(()).unwrap();
            paused.recv_timeout(Duration::from_secs(3)).unwrap();
        }
        update(Ok(snapshot(true)))
    });
    observing.recv_timeout(Duration::from_secs(3)).unwrap();
    worker
        .submit(Mutation::Setting(Setting::Stealth, true))
        .unwrap();
    // Force the exact race: shutdown disconnects the display before the poll
    // completes, while the accepted mutation is still in the command queue.
    worker.updates.take();
    release.send(()).unwrap();
    drop(worker);
    assert!(completed.load(Ordering::Relaxed));
}

#[test]
fn termination_report_is_retained_when_observation_fails() {
    let completed = Arc::new(AtomicBool::new(false));
    let worker_completed = Arc::clone(&completed);
    let mut worker = worker_with(move |work| match work {
        Some(Work::Terminate(request)) => {
            worker_completed.store(true, Ordering::Relaxed);
            assert!(request.targets.is_empty());
            Update {
                termination: Some(TerminationReport {
                    attempted: 2,
                    delivered: vec![201, 204],
                    failures: Vec::new(),
                }),
                ..update(Err(anyhow::anyhow!("observation unavailable")))
            }
        }
        None => update(Ok(snapshot(false))),
        _ => panic!("unexpected work"),
    });
    worker
        .terminate(TerminationRequest {
            targets: Vec::new(),
            mode: TerminationMode::Terminate,
        })
        .unwrap();
    assert!(worker.update_geoip().is_err());
    let result = wait_for(&mut worker, UpdateKind::Terminate);
    assert_eq!(result.termination.unwrap().delivered, vec![201, 204]);
    assert!(result.result.is_err());
    assert!(!worker.in_flight);
    completed.store(false, Ordering::Relaxed);
    worker
        .terminate(TerminationRequest {
            targets: Vec::new(),
            mode: TerminationMode::ForceKill,
        })
        .unwrap();
    drop(worker);
    assert!(completed.load(Ordering::Relaxed));
}

#[test]
fn country_update_result_is_retained_and_clears_pending_work() {
    let mut worker = worker_with(|work| {
        assert!(matches!(work, None | Some(Work::GeoIp)));
        update(Ok(Snapshot {
            geoip: Some("Validated country database".into()),
            ..snapshot(false)
        }))
    });
    worker.update_geoip().unwrap();
    assert!(worker.update_geoip().is_err());
    let result = wait_for(&mut worker, UpdateKind::GeoIp);
    assert_eq!(
        result.result.unwrap().geoip.as_deref(),
        Some("Validated country database")
    );
    assert!(!worker.in_flight);
}

#[test]
fn idle_shutdown_cancels_observation_work() {
    let worker = worker_with(|_| update(Ok(snapshot(false))));
    let cancel = Arc::clone(&worker.cancel);
    drop(worker);
    assert!(cancel.load(Ordering::Relaxed));
}

#[test]
fn accepted_profile_work_finishes_when_display_closes() {
    let completed = Arc::new(AtomicBool::new(false));
    let worker_completed = Arc::clone(&completed);
    let mut worker = worker_with(move |work| {
        if let Some(Work::Profile(ProfileOperation::Export(name))) = work {
            assert_eq!(name, "Home");
            worker_completed.store(true, Ordering::Relaxed);
        }
        update(Ok(snapshot(false)))
    });
    worker
        .profile(ProfileOperation::Export("Home".into()))
        .unwrap();
    drop(worker);
    assert!(completed.load(Ordering::Relaxed));
}

#[test]
fn profile_response_retains_operation_kind_and_payload() {
    let mut worker = worker_with(|work| Update {
        profile: match work {
            Some(Work::Profile(ProfileOperation::List)) => {
                Some(ProfileOutcome::Listed(vec!["Home".into()]))
            }
            None => None,
            _ => panic!("unexpected work"),
        },
        ..update(Ok(snapshot(false)))
    });
    worker.profile(ProfileOperation::List).unwrap();
    assert!(worker.update_geoip().is_err());
    let result = wait_for(&mut worker, UpdateKind::Profile);
    assert!(matches!(result.profile, Some(ProfileOutcome::Listed(names)) if names == ["Home"]));
    assert!(!worker.in_flight);
}

#[test]
fn failed_operation_still_reads_back_partial_changes() {
    let mut observed = false;
    let update = observe_after_work(
        UpdateKind::Firewall,
        Err(anyhow::anyhow!("second application change failed")),
        || {
            observed = true;
            Ok(snapshot(true))
        },
    );
    assert!(observed);
    assert!(update.result.unwrap().firewall.unwrap().stealth);
    assert!(
        update
            .operation_error
            .unwrap()
            .to_string()
            .contains("second application")
    );
}

#[test]
fn action_error_and_failed_readback_are_both_retained() {
    let update = observe_after_work(
        UpdateKind::Profile,
        Err(anyhow::anyhow!("restoration failed")),
        || Err(anyhow::anyhow!("observation failed")),
    );
    assert_eq!(
        update.operation_error.unwrap().to_string(),
        "restoration failed"
    );
    assert_eq!(update.result.unwrap_err().to_string(), "observation failed");
}
