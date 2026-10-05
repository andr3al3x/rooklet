use super::{
    engine::{Counters, Engine, PROCESS_LIMIT, ResourceSystem},
    *,
};
use anyhow::{Result, bail};
use rooklet_core::{model::ProcessActivity, resources::*};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    sync::atomic::AtomicBool,
    time::Duration,
};

#[derive(Default)]
struct Fake {
    now: Cell<Duration>,
    cost: Duration,
    entries: BTreeMap<u32, ProcessIdentity>,
    counters: BTreeMap<u32, Counters>,
    failures: BTreeSet<u32>,
    capture_failure: BTreeSet<u32>,
    verification_failure: BTreeSet<u32>,
    enumeration_failure: bool,
    limited: bool,
    enumerations: usize,
    captures: usize,
    reads: Vec<u32>,
    change_after_read: Option<u32>,
}
impl Fake {
    fn tick(&self) {
        self.now.set(self.now.get() + self.cost);
    }
    fn advance(&self, seconds: u64) {
        self.now.set(self.now.get() + Duration::from_secs(seconds));
    }
    fn insert(&mut self, pid: u32, bundle: Option<&str>) {
        self.entries.insert(pid, identity(pid, bundle));
        self.counters.insert(pid, counters(0, 0, 0));
    }
}
impl ResourceSystem for Fake {
    fn now(&self) -> Duration {
        self.now.get()
    }
    fn enumerate(&mut self, limit: usize) -> Result<(Vec<u32>, bool)> {
        self.tick();
        self.enumerations += 1;
        if self.enumeration_failure {
            bail!("denied");
        }
        Ok((
            self.entries.keys().copied().take(limit).collect(),
            self.limited || self.entries.len() > limit,
        ))
    }
    fn capture(&mut self, pid: u32) -> Result<ProcessIdentity> {
        self.tick();
        self.captures += 1;
        if self.capture_failure.contains(&pid) {
            bail!("capture denied");
        }
        self.entries
            .get(&pid)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("gone"))
    }
    fn matches(&mut self, identity: &ProcessIdentity) -> Result<bool> {
        self.tick();
        if self.verification_failure.contains(&identity.pid) {
            bail!("identity denied");
        }
        Ok(self.entries.get(&identity.pid) == Some(identity))
    }
    fn counters(&mut self, pid: u32) -> Result<Counters> {
        self.tick();
        self.reads.push(pid);
        if self.change_after_read == Some(pid) {
            self.entries.get_mut(&pid).unwrap().pid_version += 1;
            self.change_after_read = None;
        }
        if self.failures.contains(&pid) {
            bail!("read denied");
        }
        self.counters
            .get(&pid)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("gone"))
    }
}
fn identity(pid: u32, bundle: Option<&str>) -> ProcessIdentity {
    ProcessIdentity {
        pid,
        uid: 501,
        parent_pid: 1,
        start_sec: 10,
        start_usec: 0,
        pid_version: 1,
        path: format!("/test/{pid}"),
        bundle_path: bundle.map(str::to_owned),
    }
}
fn counters(cpu_ticks: u64, read: u64, write: u64) -> Counters {
    Counters {
        user_ticks: cpu_ticks,
        system_ticks: 0,
        nanoseconds_per_tick: 2.0,
        memory: 100,
        read,
        write,
    }
}
fn interest(pids: &[u32]) -> ResourceInterest {
    ResourceInterest {
        enabled: true,
        tracked_pids: pids.to_vec(),
        priority_pids: vec![],
    }
}
fn observe(engine: &mut Engine<Fake>, pids: &[u32]) -> Resources {
    engine.observe(&interest(pids), &AtomicBool::new(false))
}
#[test]
fn cadence_and_mach_timebase_rates_are_monotonic_and_single_core() {
    let mut engine = Engine::<Fake>::default();
    engine.system.insert(10, None);
    let first = observe(&mut engine, &[10]);
    assert_eq!(first.groups["pid:10"].state, ReadingState::WarmingUp);
    assert_eq!(first.groups["pid:10"].cpu_percent, None);
    assert_eq!(first.groups["pid:10"].memory_bytes, Some(100));
    engine.system.advance(1);
    observe(&mut engine, &[10]);
    assert_eq!(engine.system.reads, [10]);
    engine.system.advance(1);
    engine
        .system
        .counters
        .insert(10, counters(1_000_000_000, 1000, 2000));
    let second = observe(&mut engine, &[10]);
    assert_eq!(second.groups["pid:10"].cpu_percent, Some(100.0));
    assert_eq!(second.groups["pid:10"].read_per_sec, Some(500));
    assert_eq!(second.groups["pid:10"].write_per_sec, Some(1000));
    assert_eq!(engine.system.enumerations, 1);
    assert_eq!(engine.system.captures, 1);
    engine.system.advance(3);
    observe(&mut engine, &[10]);
    assert_eq!(engine.system.enumerations, 2);
    assert_eq!(engine.system.captures, 1);
}
#[test]
fn verified_helpers_are_sampled_but_unrelated_processes_are_not() {
    let mut engine = Engine::<Fake>::default();
    engine.system.insert(10, Some("/App.app"));
    engine.system.insert(11, Some("/App.app"));
    engine.system.insert(12, Some("/Other.app"));
    let resources = observe(&mut engine, &[10]);
    assert_eq!(engine.system.reads, [10, 11]);
    assert_eq!(resources.groups["/App.app"].process_count, 2);
    assert!(!resources.groups.contains_key("/Other.app"));
    let captures = engine.system.captures;
    engine.system.advance(2);
    observe(&mut engine, &[]);
    assert_eq!(engine.system.reads.len(), 2);
    assert_eq!(engine.system.captures, captures);
}
#[test]
fn failures_are_partial_then_stale_and_do_not_fabricate_rates() {
    let mut engine = Engine::<Fake>::default();
    engine.system.insert(10, Some("/App.app"));
    engine.system.insert(11, Some("/App.app"));
    engine.system.failures.insert(11);
    let resources = observe(&mut engine, &[10]);
    let usage = &resources.groups["/App.app"];
    assert_eq!(usage.state, ReadingState::Partial);
    assert_eq!(usage.sampled_count, 1);
    assert_eq!(usage.cpu_percent, None);
    assert_eq!(usage.memory_bytes, Some(100));
    assert_eq!(usage.processes[1].state, ReadingState::Unavailable);
    engine.system.advance(2);
    engine.system.failures.insert(10);
    let failed = observe(&mut engine, &[10]);
    assert_eq!(
        failed.groups["/App.app"].processes[0].state,
        ReadingState::Stale
    );
    engine.system.advance(2);
    engine.system.failures.clear();
    let resumed = observe(&mut engine, &[10]);
    assert_eq!(resumed.groups["/App.app"].state, ReadingState::WarmingUp);
    assert_eq!(resumed.groups["/App.app"].cpu_percent, None);
}
#[test]
fn counter_decreases_reset_all_rates() {
    let mut engine = Engine::<Fake>::default();
    engine.system.insert(10, None);
    engine.system.counters.insert(10, counters(100, 100, 100));
    observe(&mut engine, &[10]);
    engine.system.advance(2);
    engine.system.counters.insert(10, counters(200, 99, 200));
    let resources = observe(&mut engine, &[10]);
    assert_eq!(resources.groups["pid:10"].state, ReadingState::WarmingUp);
    assert_eq!(resources.groups["pid:10"].cpu_percent, None);
    assert_eq!(resources.groups["pid:10"].write_per_sec, None);
}
#[test]
fn disabled_discovery_continues_and_resume_warms_up() {
    let mut engine = Engine::<Fake>::default();
    engine.system.insert(10, None);
    observe(&mut engine, &[10]);
    engine.system.advance(2);
    let disabled = ResourceInterest {
        enabled: false,
        ..interest(&[10])
    };
    assert!(!engine.observe(&disabled, &AtomicBool::new(false)).enabled);
    engine.system.insert(11, None);
    engine.system.advance(3);
    engine.observe(&disabled, &AtomicBool::new(false));
    assert_eq!(engine.identities().len(), 2);
    assert_eq!(engine.system.reads, [10]);
    let resumed = observe(&mut engine, &[10]);
    assert_eq!(resumed.groups["pid:10"].state, ReadingState::WarmingUp);
    assert_eq!(resumed.groups["pid:10"].cpu_percent, None);
}
#[test]
fn reuse_exec_and_mid_read_exec_discard_old_baseline_and_group() {
    let mut engine = Engine::<Fake>::default();
    engine.system.insert(10, Some("/Old.app"));
    observe(&mut engine, &[10]);
    engine.system.advance(2);
    let changed = engine.system.entries.get_mut(&10).unwrap();
    changed.pid_version += 1;
    changed.path = "/new/executable".into();
    changed.bundle_path = Some("/New.app".into());
    let resources = observe(&mut engine, &[10]);
    assert!(!resources.groups.contains_key("/Old.app"));
    assert_eq!(resources.groups["/New.app"].state, ReadingState::WarmingUp);
    assert_eq!(engine.system.captures, 2);
    engine.system.advance(2);
    engine.system.change_after_read = Some(10);
    let changed = observe(&mut engine, &[10]);
    assert_eq!(changed.groups["/New.app"].state, ReadingState::WarmingUp);
    assert_eq!(changed.groups["/New.app"].cpu_percent, None);
    assert_eq!(engine.system.captures, 3);
}
#[test]
fn helper_exits_are_removed_and_failed_enumeration_expires_cache() {
    let mut engine = Engine::<Fake>::default();
    engine.system.insert(10, Some("/App.app"));
    engine.system.insert(11, Some("/App.app"));
    observe(&mut engine, &[10]);
    engine.system.entries.remove(&11);
    engine.system.advance(5);
    let resources = observe(&mut engine, &[10]);
    assert_eq!(resources.groups["/App.app"].process_count, 1);
    engine.system.enumeration_failure = true;
    engine.system.advance(31);
    let resources = observe(&mut engine, &[10]);
    assert!(resources.groups.is_empty());
    assert!(engine.identities().is_empty());
    assert!(resources.message.unwrap().contains("discovery unavailable"));
}
#[test]
fn budget_bounds_incremental_capture_and_reports_partial_coverage() {
    let mut engine = Engine::<Fake>::default();
    engine.system.cost = Duration::from_millis(5);
    for pid in 10..30 {
        engine.system.insert(pid, None);
    }
    let disabled = ResourceInterest::default();
    let before = engine.system.now.get();
    engine.observe(&disabled, &AtomicBool::new(false));
    let first_captures = engine.system.captures;
    assert!(first_captures > 0 && first_captures < 20);
    assert!(engine.system.now.get() - before <= Duration::from_millis(20));
    let before = engine.system.now.get();
    engine.observe(&disabled, &AtomicBool::new(false));
    assert!(engine.system.captures > first_captures);
    assert!(engine.system.now.get() - before <= Duration::from_millis(20));
    let resources = observe(&mut engine, &[10]);
    assert!(resources.message.unwrap().contains("coverage is partial"));
    assert!(engine.system.captures < 20);
}
#[test]
fn fair_rotation_progresses_despite_selected_process_priority() {
    let mut engine = Engine::<Fake>::default();
    for pid in 10..20 {
        engine.system.insert(pid, None);
    }
    engine.observe(&ResourceInterest::default(), &AtomicBool::new(false));
    engine.system.cost = Duration::from_millis(5);
    let interest = ResourceInterest {
        priority_pids: vec![10],
        ..interest(&(10..20).collect::<Vec<_>>())
    };
    for _ in 0..15 {
        engine.observe(&interest, &AtomicBool::new(false));
        engine.system.advance(2);
    }
    assert_eq!(
        engine.system.reads.iter().copied().collect::<BTreeSet<_>>(),
        (10..20).collect()
    );
}
#[test]
fn cancellation_skips_native_work_and_cache_limit_is_explicit() {
    let mut engine = Engine::<Fake>::default();
    for pid in 10..(10 + PROCESS_LIMIT as u32 + 2) {
        engine.system.insert(pid, None);
    }
    engine.observe(&interest(&[10]), &AtomicBool::new(true));
    assert_eq!(engine.system.enumerations, 0);
    let resources = observe(&mut engine, &[10]);
    assert_eq!(engine.identities().len(), PROCESS_LIMIT);
    assert!(resources.message.unwrap().contains("4096-process limit"));
}
#[test]
fn attribution_requires_captured_generation_even_for_the_same_path() {
    let mut engine = Engine::<Fake>::default();
    engine.system.insert(10, None);
    let resources = observe(&mut engine, &[10]);
    let mut activity = ProcessActivity {
        pid: 10,
        name: "test".into(),
        path: Some("/test/10".into()),
        identities: vec![],
        bytes_in: 0,
        bytes_out: 0,
        rate_in: 0,
        rate_out: 0,
        connections: vec![],
    };
    assert!(resources.for_activity(&activity).is_none());
    activity.path = Some("/other".into());
    assert!(resources.for_activity(&activity).is_none());
    activity.identities = vec![identity(10, None)];
    assert!(resources.for_activity(&activity).is_some());
    activity.identities[0].pid_version += 1;
    assert!(resources.for_activity(&activity).is_none());
}

#[test]
fn failed_generation_verification_cannot_be_kept_alive_by_enumeration() {
    let mut engine = Engine::<Fake>::default();
    engine.system.insert(10, None);
    engine.observe(&ResourceInterest::default(), &AtomicBool::new(false));
    engine.system.verification_failure.insert(10);
    engine.system.capture_failure.insert(10);
    for _ in 0..7 {
        engine.system.advance(5);
        engine.observe(&ResourceInterest::default(), &AtomicBool::new(false));
    }
    assert!(engine.identities().is_empty());
}
#[test]
fn age_marks_cached_values_stale_without_fabricating_a_new_observation() {
    let mut engine = Engine::<Fake>::default();
    engine.system.insert(10, None);
    observe(&mut engine, &[10]);
    engine.system.advance(7);
    let resources = engine.observe(&interest(&[10]), &AtomicBool::new(true));
    assert_eq!(resources.groups["pid:10"].state, ReadingState::Stale);
    assert_eq!(resources.groups["pid:10"].age_ms, Some(7000));
    assert_eq!(resources.groups["pid:10"].sampled_count, 0);
}
#[cfg(target_os = "macos")]
#[test]
fn native_spawned_process_identity_memory_and_cpu_counters_are_available() {
    use std::{
        process::{Child, Command, Stdio},
        thread,
        time::Instant,
    };
    struct OwnedChild(Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let child = OwnedChild(
        Command::new("/usr/bin/yes")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let pid = child.0.id();
    let mut native = super::native::Native::default();
    let identity = native.capture(pid).unwrap();
    assert!(native.matches(&identity).unwrap());
    let before = native.counters(pid).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let after = loop {
        let after = native.counters(pid).unwrap();
        assert!(after.user_ticks >= before.user_ticks);
        assert!(after.system_ticks >= before.system_ticks);
        if after.user_ticks > before.user_ticks || after.system_ticks > before.system_ticks {
            break after;
        }
        assert!(
            Instant::now() < deadline,
            "test child CPU counters made no progress"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert!(native.matches(&identity).unwrap());
    assert!(after.memory > 0);
    assert!(after.nanoseconds_per_tick.is_finite() && after.nanoseconds_per_tick > 0.0);
}
#[cfg(target_os = "macos")]
#[test]
#[ignore = "manual read-only native observation cost diagnostic (about 12 seconds)"]
fn native_observation_cost_diagnostic() {
    use std::{thread, time::Instant};
    for enabled in [false, true] {
        let mut sampler = Sampler::default();
        let interest = ResourceInterest {
            enabled,
            tracked_pids: vec![std::process::id()],
            priority_pids: vec![std::process::id()],
        };
        let mut calls = Vec::new();
        let mut groups = 0;
        for _ in 0..6 {
            let started = Instant::now();
            groups = sampler
                .observe(&interest, &AtomicBool::new(false))
                .groups
                .len();
            calls.push(started.elapsed());
            thread::sleep(Duration::from_secs(1));
        }
        eprintln!(
            "native resource enabled={enabled}: calls_us={:?}, cached_identities={}, observed_groups={groups}",
            calls.iter().map(Duration::as_micros).collect::<Vec<_>>(),
            sampler.identities().len()
        );
    }
}

#[test]
fn selected_and_normal_processes_progress_with_a_single_read_budget() {
    let mut engine = Engine::<Fake>::default();
    for pid in 10..30 {
        engine.system.insert(pid, None);
    }
    engine.observe(&ResourceInterest::default(), &AtomicBool::new(false));
    // A reading has two generation checks and one counter read: 21 ms.
    engine.system.cost = Duration::from_millis(7);
    let interest = ResourceInterest {
        priority_pids: vec![10],
        ..interest(&(10..30).collect::<Vec<_>>())
    };
    for _ in 0..8 {
        let before = engine.system.reads.len();
        engine.observe(&interest, &AtomicBool::new(false));
        assert!(engine.system.reads.len() - before <= 1);
        engine.system.advance(2);
    }
    let selected_reads = engine.system.reads.iter().filter(|pid| **pid == 10).count();
    assert!(
        selected_reads >= 2,
        "selected PID was starved: {:?}",
        engine.system.reads
    );
    assert!(
        engine
            .system
            .reads
            .iter()
            .filter(|pid| **pid != 10)
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            >= 2,
        "normal rotation was starved: {:?}",
        engine.system.reads
    );
}
#[test]
fn discovery_and_counters_progress_when_one_operation_consumes_the_budget() {
    let mut engine = Engine::<Fake>::default();
    engine.system.insert(10, None);
    engine.observe(&ResourceInterest::default(), &AtomicBool::new(false));
    for pid in 11..100 {
        engine.system.insert(pid, None);
    }
    engine.system.advance(5);
    // Each capture now takes a full budget; a reading takes three slots.
    engine.system.cost = Duration::from_millis(21);
    for _ in 0..8 {
        observe(&mut engine, &[10]);
        engine.system.advance(2);
    }
    assert!(engine.system.captures >= 2, "discovery made no progress");
    assert!(
        engine.system.captures < 90,
        "test must still have a discovery backlog"
    );
    assert!(
        engine.system.reads.len() >= 2,
        "counter reads were starved by discovery"
    );
}
