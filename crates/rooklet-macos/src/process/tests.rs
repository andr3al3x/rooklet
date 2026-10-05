use super::engine::ProcessSystem;
use super::*;
use anyhow::bail;
use rooklet_core::process::TerminationMode;
use rooklet_core::process::{MAX_TARGETS, members_for};
use std::{
    collections::{HashMap, VecDeque},
    fs,
};

fn identity(pid: u32) -> ProcessIdentity {
    ProcessIdentity {
        pid,
        uid: 501,
        parent_pid: 1,
        start_sec: 1000,
        start_usec: 20,
        pid_version: 3,
        path: "/bin/sleep".into(),
        bundle_path: None,
    }
}
struct Fake {
    identities: HashMap<u32, ProcessIdentity>,
    changes: HashMap<u32, VecDeque<ProcessIdentity>>,
    parents: HashMap<u32, u32>,
    signals: Vec<(u32, TerminationMode)>,
    failures: Vec<u32>,
    uid: u32,
    pids: Vec<u32>,
}
impl Fake {
    fn new(targets: &[ProcessIdentity]) -> Self {
        Self {
            identities: targets
                .iter()
                .map(|target| (target.pid, target.clone()))
                .collect(),
            changes: HashMap::new(),
            parents: HashMap::from([(100, 50), (50, 1)]),
            signals: Vec::new(),
            failures: Vec::new(),
            uid: 501,
            pids: targets.iter().map(|target| target.pid).collect(),
        }
    }
}
impl ProcessSystem for Fake {
    fn uid(&self) -> u32 {
        self.uid
    }
    fn own_pid(&self) -> u32 {
        100
    }
    fn parent_pid(&mut self, pid: u32) -> Result<u32> {
        self.parents
            .get(&pid)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("ancestor unavailable"))
    }
    fn list_pids(&mut self) -> Result<Vec<u32>> {
        Ok(self.pids.clone())
    }
    fn capture(&mut self, pid: u32) -> Result<ProcessIdentity> {
        if let Some(queue) = self.changes.get_mut(&pid)
            && let Some(identity) = queue.pop_front()
        {
            return Ok(identity);
        }
        self.identities
            .get(&pid)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("process exited"))
    }
    fn signal(&mut self, identity: &ProcessIdentity, mode: TerminationMode) -> Result<()> {
        self.signals.push((identity.pid, mode));
        if self.failures.contains(&identity.pid) {
            bail!("OS rejected signal");
        }
        Ok(())
    }
}
fn request(targets: Vec<ProcessIdentity>, mode: TerminationMode) -> TerminationRequest {
    TerminationRequest { targets, mode }
}
#[test]
fn whole_request_preflight_rejects_reuse_and_sends_nothing() {
    let targets = vec![identity(200), identity(201)];
    for field in 0..5 {
        let mut fake = Fake::new(&targets);
        let changed = fake.identities.get_mut(&201).unwrap();
        match field {
            0 => changed.uid += 1,
            1 => changed.start_sec += 1,
            2 => changed.start_usec += 1,
            3 => changed.path = "/bin/different".into(),
            _ => changed.pid_version += 1,
        }
        assert!(
            engine::terminate(
                &mut fake,
                &request(targets.clone(), TerminationMode::Terminate)
            )
            .is_err()
        );
        assert!(fake.signals.is_empty());
    }
}
#[test]
fn protected_self_ancestors_system_owner_and_other_user_are_rejected() {
    for target in [
        identity(0),
        identity(1),
        identity(100),
        identity(50),
        ProcessIdentity {
            uid: 0,
            ..identity(200)
        },
        ProcessIdentity {
            uid: 502,
            ..identity(200)
        },
    ] {
        let mut fake = Fake::new(std::slice::from_ref(&target));
        assert!(
            engine::terminate(
                &mut fake,
                &request(vec![target], TerminationMode::ForceKill)
            )
            .is_err()
        );
        assert!(fake.signals.is_empty());
    }
    let target = identity(200);
    let mut fake = Fake::new(std::slice::from_ref(&target));
    fake.uid = 0;
    assert!(
        engine::terminate(
            &mut fake,
            &request(vec![target], TerminationMode::Terminate)
        )
        .is_err()
    );
    assert!(fake.signals.is_empty());
}
#[test]
fn empty_duplicate_oversized_and_unverifiable_ancestry_fail_closed() {
    let target = identity(200);
    for targets in [
        Vec::new(),
        vec![target.clone(), target.clone()],
        vec![target.clone(); MAX_TARGETS + 1],
    ] {
        let mut fake = Fake::new(&targets);
        assert!(
            engine::terminate(&mut fake, &request(targets, TerminationMode::Terminate)).is_err()
        );
        assert!(fake.signals.is_empty());
    }
    for parent in [None, Some(100)] {
        let mut fake = Fake::new(std::slice::from_ref(&target));
        match parent {
            None => {
                fake.parents.remove(&50);
            }
            Some(pid) => {
                fake.parents.insert(50, pid);
            }
        }
        assert!(
            engine::terminate(
                &mut fake,
                &request(vec![target.clone()], TerminationMode::Terminate)
            )
            .is_err()
        );
        assert!(fake.signals.is_empty());
    }
}
#[test]
fn signals_only_captured_targets_and_reports_delivery_not_exit() {
    let targets = vec![identity(200), identity(201)];
    let mut fake = Fake::new(&targets);
    fake.identities.insert(202, identity(202));
    let report =
        engine::terminate(&mut fake, &request(targets, TerminationMode::Terminate)).unwrap();
    assert_eq!(report.attempted, 2);
    assert_eq!(report.delivered, vec![200, 201]);
    assert!(report.failures.is_empty());
    assert_eq!(
        fake.signals,
        vec![
            (200, TerminationMode::Terminate),
            (201, TerminationMode::Terminate)
        ]
    );
}
#[test]
fn per_signal_identity_race_and_os_error_produce_explicit_partial_report() {
    let targets = vec![identity(200), identity(201), identity(202)];
    let mut fake = Fake::new(&targets);
    let mut reused = identity(201);
    reused.pid_version += 1;
    fake.changes
        .insert(201, VecDeque::from([identity(201), reused]));
    fake.failures.push(202);
    let report =
        engine::terminate(&mut fake, &request(targets, TerminationMode::ForceKill)).unwrap();
    assert_eq!(report.delivered, vec![200]);
    assert_eq!(
        report
            .failures
            .iter()
            .map(|failure| failure.pid)
            .collect::<Vec<_>>(),
        [201, 202]
    );
    assert_eq!(
        fake.signals,
        vec![
            (200, TerminationMode::ForceKill),
            (202, TerminationMode::ForceKill)
        ]
    );
}
#[test]
fn snapshots_are_bounded_deduplicated_and_current_user_only() {
    let targets = vec![identity(200), identity(201)];
    let mut fake = Fake::new(&targets);
    fake.pids = vec![0, 1, 200, 200, 201, 202];
    fake.identities.get_mut(&201).unwrap().uid = 502;
    assert_eq!(
        engine::capture_snapshot(&mut fake).unwrap(),
        vec![identity(200)]
    );
    fake.pids = vec![200; MAX_PROCESSES];
    assert!(engine::capture_snapshot(&mut fake).is_err());
    fake.uid = 0;
    fake.pids = vec![200];
    fake.identities.get_mut(&200).unwrap().uid = 0;
    assert!(engine::capture_snapshot(&mut fake).unwrap().is_empty());
}
#[test]
fn outer_verified_bundle_groups_nested_helpers_without_name_guessing() {
    let directory = tempfile::tempdir().unwrap();
    let app = directory.path().join("Example.app");
    let helper = app.join("Contents/Helpers/Nested.app/Contents/MacOS/helper");
    fs::create_dir_all(helper.parent().unwrap()).unwrap();
    fs::write(&helper, "fixture").unwrap();
    fs::create_dir_all(app.join("Contents")).unwrap();
    fs::write(app.join("Contents/Info.plist"), "fixture").unwrap();
    fs::write(
        app.join("Contents/Helpers/Nested.app/Contents/Info.plist"),
        "fixture",
    )
    .unwrap();
    assert_eq!(verified_bundle(&helper), Some(app.clone()));
    let main = app.join("Contents/MacOS/main");
    fs::create_dir_all(main.parent().unwrap()).unwrap();
    fs::write(&main, "fixture").unwrap();
    let mut first = identity(200);
    first.path = main.to_string_lossy().into_owned();
    first.bundle_path = Some(app.to_string_lossy().into_owned());
    let mut second = identity(201);
    second.path = helper.to_string_lossy().into_owned();
    second.bundle_path = first.bundle_path.clone();
    let unrelated = identity(202);
    assert_eq!(
        members_for(&[first.clone(), second.clone(), unrelated.clone()], 200),
        vec![first, second]
    );
    assert_eq!(
        members_for(std::slice::from_ref(&unrelated), 202),
        vec![unrelated]
    );
    assert!(verified_bundle(Path::new("relative.app/Contents/main")).is_none());
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::{
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };
    struct OwnedChild(Child);
    impl OwnedChild {
        fn spawn() -> Self {
            Self(
                Command::new("/bin/sleep")
                    .arg("30")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            )
        }
        fn wait_bounded(&mut self) {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                if self.0.try_wait().unwrap().is_some() {
                    return;
                }
                assert!(Instant::now() < deadline, "test child did not exit");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    #[test]
    fn actual_signals_are_delivered_only_to_test_owned_children() {
        for mode in [TerminationMode::Terminate, TerminationMode::ForceKill] {
            let mut child = OwnedChild::spawn();
            let captured = capture(child.0.id()).unwrap();
            assert_eq!(captured.pid, child.0.id());
            assert_eq!(captured.path, "/bin/sleep");
            assert_eq!(process_path(child.0.id()), Some("/bin/sleep".into()));
            let report = terminate(&request(vec![captured], mode)).unwrap();
            assert_eq!(report.delivered, vec![child.0.id()]);
            assert!(report.failures.is_empty());
            child.wait_bounded();
        }
    }
    #[test]
    fn native_bundle_capture_includes_non_network_helpers_and_signals_exact_members() {
        let directory = tempfile::tempdir().unwrap();
        let bundle = directory.path().join("ChildFixture.app");
        let main_path = bundle.join("Contents/MacOS/main");
        let helper_path = bundle.join("Contents/Helpers/Inner.app/Contents/MacOS/helper");
        for path in [&main_path, &helper_path] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::copy("/bin/sleep", path).unwrap();
        }
        fs::write(bundle.join("Contents/Info.plist"), "test fixture").unwrap();
        fs::write(
            bundle.join("Contents/Helpers/Inner.app/Contents/Info.plist"),
            "test fixture",
        )
        .unwrap();
        let mut main = OwnedChild(Command::new(&main_path).arg("30").spawn().unwrap());
        let mut helper = OwnedChild(Command::new(&helper_path).arg("30").spawn().unwrap());
        let snapshot = capture_snapshot().unwrap();
        let members = members_for(&snapshot, main.0.id());
        assert_eq!(members.len(), 2);
        let bundle = bundle.canonicalize().unwrap();
        assert!(
            members
                .iter()
                .all(|member| member.bundle_path.as_deref() == bundle.to_str())
        );
        assert!(members.iter().any(|member| member.pid == helper.0.id()));
        let report = terminate(&request(members, TerminationMode::Terminate)).unwrap();
        assert_eq!(report.delivered.len(), 2);
        assert!(report.failures.is_empty());
        main.wait_bounded();
        helper.wait_bounded();
    }
    #[test]
    fn audit_signal_rejects_wrong_generation_in_kernel_and_leaves_child_alive() {
        let mut child = OwnedChild::spawn();
        let mut captured = capture(child.0.id()).unwrap();
        captured.pid_version = captured.pid_version.wrapping_add(1);
        assert!(
            native::Native
                .signal(&captured, TerminationMode::ForceKill)
                .is_err()
        );
        assert!(child.0.try_wait().unwrap().is_none());
    }
    #[test]
    fn native_capture_snapshot_includes_a_test_child_without_network_activity() {
        let child = OwnedChild::spawn();
        let snapshot = capture_snapshot().unwrap();
        assert!(snapshot.iter().any(|process| process.pid == child.0.id()));
    }
}

#[test]
fn signal_logs_partial_outcomes_without_process_identity() {
    let logs = crate::command::regression_tests::capture_logs(|| {
        let mut target = identity(123456789);
        target.path = "/PRIVATE_PROCESS_PATH_30d1".into();
        target.bundle_path = Some("/PRIVATE_BUNDLE_PATH_110d".into());
        let mut system = Fake::new(std::slice::from_ref(&target));
        system.failures.push(target.pid);
        let report = engine::terminate(
            &mut system,
            &TerminationRequest {
                targets: vec![target],
                mode: TerminationMode::Terminate,
            },
        )
        .unwrap();
        assert_eq!(report.failures.len(), 1);
    });
    assert!(logs.contains("WARN"));
    assert!(logs.contains("outcome=\"partial\""));
    assert!(logs.contains("failed_count=1"));
    assert!(!logs.contains("PRIVATE_"));
    assert!(!logs.contains("123456789"));
    assert!(!logs.contains("OS rejected signal"));
}
