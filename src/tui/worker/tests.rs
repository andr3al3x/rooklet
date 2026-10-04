use super::*;
use xield::model::Setting;

#[test]
fn queued_observation_does_not_drop_mutation_result() {
    let mut backend = Backend::new(true).unwrap();
    let (responses, updates) = mpsc::sync_channel(1);
    assert!(publish(
        &responses,
        Update {
            result: backend.snapshot(),
            kind: UpdateKind::Observation,
            termination: None,
        }
    ));
    // Another observation is skipped while the bounded queue is full.
    assert!(publish(
        &responses,
        Update {
            result: backend.snapshot(),
            kind: UpdateKind::Observation,
            termination: None,
        }
    ));
    backend
        .mutate(Mutation::Setting(Setting::Stealth, true))
        .unwrap();
    let thread = thread::spawn(move || {
        publish(
            &responses,
            Update {
                result: backend.snapshot(),
                kind: UpdateKind::Firewall,
                termination: None,
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
    let (finished, completion) = mpsc::channel();
    let thread = thread::spawn(move || {
        let mut worker = Worker::start(true).unwrap();
        worker
            .submit(Mutation::Setting(Setting::Stealth, true))
            .unwrap();
        drop(worker);
        finished.send(()).unwrap();
    });
    completion.recv_timeout(Duration::from_secs(3)).unwrap();
    thread.join().unwrap();
}

#[test]
fn shutdown_during_observation_finishes_the_accepted_mutation() {
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let completed = Arc::new(AtomicBool::new(false));
    let worker_completed = Arc::clone(&completed);
    let (commands, work) = mpsc::sync_channel(1);
    let (responses, updates) = mpsc::sync_channel(1);
    let (started, observing) = mpsc::channel();
    let (release, paused) = mpsc::channel();
    let thread = thread::spawn(move || {
        let mut backend = Backend::new(true).unwrap();
        observe(work, responses, &worker_cancel, |work| {
            if let Some(work) = work {
                work.run(&mut backend).unwrap();
                worker_completed.store(true, Ordering::Relaxed);
            } else {
                started.send(()).unwrap();
                paused.recv_timeout(Duration::from_secs(3)).unwrap();
            }
            Update {
                result: backend.snapshot(),
                kind: UpdateKind::Observation,
                termination: None,
            }
        });
    });
    let mut worker = Worker {
        commands: Some(commands),
        updates: Some(updates),
        cancel,
        thread: Some(thread),
        in_flight: false,
    };
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
fn termination_result_is_retained_and_worker_finishes_on_shutdown() {
    let mut demo = Backend::new(true).unwrap();
    let targets = demo.snapshot().unwrap().activity[0].identities.clone();
    let mut worker = Worker::start(true).unwrap();
    worker
        .terminate(TerminationRequest {
            targets,
            mode: xield::process::TerminationMode::Terminate,
        })
        .unwrap();
    assert!(worker.update_geoip().is_err());
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(update) = worker
            .drain()
            .into_iter()
            .find(|update| update.kind == UpdateKind::Terminate)
        {
            assert_eq!(update.termination.unwrap().delivered, vec![201, 204]);
            assert!(
                update
                    .result
                    .unwrap()
                    .activity
                    .iter()
                    .all(|activity| activity.name != "Safari")
            );
            assert!(!worker.in_flight);
            break;
        }
        assert!(Instant::now() < deadline, "termination result was lost");
        thread::sleep(Duration::from_millis(5));
    }
    let targets = demo.snapshot().unwrap().activity[1].identities.clone();
    worker
        .terminate(TerminationRequest {
            targets,
            mode: xield::process::TerminationMode::ForceKill,
        })
        .unwrap();
    drop(worker);
}

#[test]
fn country_update_result_is_retained_and_completes_without_privileges_in_demo() {
    let mut worker = Worker::start(true).unwrap();
    worker.update_geoip().unwrap();
    assert!(worker.update_geoip().is_err());
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(update) = worker
            .drain()
            .into_iter()
            .find(|update| update.kind == UpdateKind::GeoIp)
        {
            let snapshot = update.result.unwrap();
            assert!(snapshot.demo);
            assert!(snapshot.firewall.unwrap().enabled);
            assert_eq!(snapshot.geoip.as_deref(), Some("Simulated country data"));
            assert!(!worker.in_flight);
            break;
        }
        assert!(Instant::now() < deadline, "country update result was lost");
        thread::sleep(Duration::from_millis(5));
    }
}
