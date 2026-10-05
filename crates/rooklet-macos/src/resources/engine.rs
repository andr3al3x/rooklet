use anyhow::Result;
use rooklet_core::process::ProcessIdentity;
use rooklet_core::resources::{ProcessReading, ReadingState, ResourceInterest, Resources, Usage};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub(super) const PROCESS_LIMIT: usize = 4096;
const PRIORITY_LIMIT: usize = 32;
const ENUMERATION_INTERVAL: Duration = Duration::from_secs(5);
const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
const FRESH_LIMIT: Duration = Duration::from_secs(6);
const CACHE_TTL: Duration = Duration::from_secs(30);
const WORK_BUDGET: Duration = Duration::from_millis(20);

#[derive(Clone, Copy)]
pub(super) struct Counters {
    pub user_ticks: u64,
    pub system_ticks: u64,
    pub nanoseconds_per_tick: f64,
    pub memory: u64,
    pub read: u64,
    pub write: u64,
}
pub(super) trait ResourceSystem {
    fn now(&self) -> Duration;
    fn enumerate(&mut self, limit: usize) -> Result<(Vec<u32>, bool)>;
    fn capture(&mut self, pid: u32) -> Result<ProcessIdentity>;
    fn matches(&mut self, identity: &ProcessIdentity) -> Result<bool>;
    fn counters(&mut self, pid: u32) -> Result<Counters>;
}
struct Entry {
    reading: ProcessReading,
    baseline: Option<(Duration, Counters)>,
    observed: Option<Duration>,
    attempted: Option<Duration>,
    last_seen: Duration,
    verified: Duration,
}
impl Entry {
    fn new(identity: ProcessIdentity, now: Duration) -> Self {
        Self {
            reading: ProcessReading {
                identity,
                cpu_percent: None,
                memory_bytes: None,
                read_per_sec: None,
                write_per_sec: None,
                state: ReadingState::Unavailable,
                age_ms: None,
            },
            baseline: None,
            observed: None,
            attempted: None,
            last_seen: now,
            verified: now,
        }
    }
    fn reset(&mut self) {
        self.baseline = None;
        self.observed = None;
        self.attempted = None;
        self.reading.cpu_percent = None;
        self.reading.memory_bytes = None;
        self.reading.read_per_sec = None;
        self.reading.write_per_sec = None;
        self.reading.state = ReadingState::Unavailable;
    }
    fn update(&mut self, now: Duration, counters: Counters) {
        let rates = self.baseline.and_then(|(before, old)| {
            let elapsed = now.checked_sub(before)?;
            if elapsed.is_zero() || elapsed > CACHE_TTL {
                return None;
            }
            let user = counters.user_ticks.checked_sub(old.user_ticks)?;
            let system = counters.system_ticks.checked_sub(old.system_ticks)?;
            let read = counters.read.checked_sub(old.read)?;
            let write = counters.write.checked_sub(old.write)?;
            let cpu = (user as f64 + system as f64) * counters.nanoseconds_per_tick
                / elapsed.as_nanos() as f64
                * 100.0;
            cpu.is_finite()
                .then_some((cpu, rate(read, elapsed), rate(write, elapsed)))
        });
        self.reading.cpu_percent = rates.map(|rates| rates.0);
        self.reading.read_per_sec = rates.map(|rates| rates.1);
        self.reading.write_per_sec = rates.map(|rates| rates.2);
        self.reading.memory_bytes = Some(counters.memory);
        self.reading.state = if rates.is_some() {
            ReadingState::Fresh
        } else {
            ReadingState::WarmingUp
        };
        self.baseline = Some((now, counters));
        self.observed = Some(now);
    }
    fn fail(&mut self) {
        self.baseline = None;
        self.reading.state = if self.observed.is_some() {
            ReadingState::Stale
        } else {
            ReadingState::Unavailable
        };
    }
    fn reading(&self, now: Duration) -> ProcessReading {
        let mut reading = self.reading.clone();
        reading.age_ms = self.observed.map(|observed| {
            now.saturating_sub(observed)
                .as_millis()
                .min(u128::from(u64::MAX)) as u64
        });
        if self
            .observed
            .is_some_and(|observed| now.saturating_sub(observed) > FRESH_LIMIT)
        {
            reading.state = ReadingState::Stale;
        }
        reading
    }
}
fn rate(bytes: u64, elapsed: Duration) -> u64 {
    (u128::from(bytes) * 1_000_000_000 / elapsed.as_nanos()).min(u128::from(u64::MAX)) as u64
}
pub(super) struct Engine<S> {
    pub(super) system: S,
    entries: BTreeMap<u32, Entry>,
    discovery: VecDeque<u32>,
    enumeration: Option<Duration>,
    cursor: u32,
    priority_turn: bool,
    discovery_turn: bool,
    enabled: bool,
    message: Option<String>,
    limited: bool,
    capture_failures: usize,
}
impl<S: Default> Default for Engine<S> {
    fn default() -> Self {
        Self {
            system: S::default(),
            entries: BTreeMap::new(),
            discovery: VecDeque::new(),
            enumeration: None,
            cursor: 0,
            priority_turn: true,
            discovery_turn: true,
            enabled: false,
            message: None,
            limited: false,
            capture_failures: 0,
        }
    }
}
impl<S: ResourceSystem> Engine<S> {
    pub(super) fn identities(&self) -> Vec<ProcessIdentity> {
        let now = self.system.now();
        self.entries
            .values()
            .filter(|entry| {
                now.saturating_sub(entry.last_seen) <= CACHE_TTL
                    && now.saturating_sub(entry.verified) <= CACHE_TTL
            })
            .map(|entry| entry.reading.identity.clone())
            .collect()
    }
    pub(super) fn observe(
        &mut self,
        interest: &ResourceInterest,
        cancel: &AtomicBool,
    ) -> Resources {
        let started = self.system.now();
        if self.enabled != interest.enabled {
            self.enabled = interest.enabled;
            for entry in self.entries.values_mut() {
                entry.reset();
            }
        }
        self.entries.retain(|_, entry| {
            started.saturating_sub(entry.last_seen) <= CACHE_TTL
                && started.saturating_sub(entry.verified) <= CACHE_TTL
        });
        if !cancel.load(Ordering::Relaxed)
            && self
                .enumeration
                .is_none_or(|last| started.saturating_sub(last) >= ENUMERATION_INTERVAL)
        {
            self.enumeration = Some(started);
            match self.system.enumerate(PROCESS_LIMIT) {
                Ok((pids, limited)) => {
                    self.message = None;
                    self.limited = limited;
                    self.capture_failures = 0;
                    let listed: BTreeSet<_> = pids
                        .into_iter()
                        .filter(|pid| *pid > 1)
                        .take(PROCESS_LIMIT)
                        .collect();
                    self.entries.retain(|pid, entry| {
                        if listed.contains(pid) {
                            entry.last_seen = started;
                            true
                        } else {
                            limited
                        }
                    });
                    self.discovery.retain(|pid| listed.contains(pid));
                    let queued: BTreeSet<_> = self.discovery.iter().copied().collect();
                    self.discovery
                        .extend(listed.into_iter().filter(|pid| !queued.contains(pid)));
                }
                Err(error) => {
                    self.message = Some(format!("Process discovery unavailable: {error}"))
                }
            }
        }
        // Capturing representatives first identifies the full bundle before
        // selecting its helpers; the existing backlog keeps its relative order.
        let tracked: BTreeSet<_> = interest
            .tracked_pids
            .iter()
            .copied()
            .take(PROCESS_LIMIT)
            .collect();
        let mut first = VecDeque::new();
        self.discovery.retain(|pid| {
            if tracked.contains(pid) && !self.entries.contains_key(pid) {
                first.push_back(*pid);
                false
            } else {
                true
            }
        });
        first.append(&mut self.discovery);
        self.discovery = first;
        let mut bundles = self.bundles_for(&tracked);
        let mut samples = if self.enabled {
            self.sample_order(interest, started)
        } else {
            VecDeque::new()
        };
        // Alternate discovery and counters: neither a helper churn backlog nor
        // visible-process priority can consume every cooperative work slot.
        while !cancel.load(Ordering::Relaxed)
            && self.system.now().saturating_sub(started) < WORK_BUDGET
            && (!self.discovery.is_empty() || !samples.is_empty())
        {
            if self.discovery_turn && !self.discovery.is_empty() || samples.is_empty() {
                self.discovery_turn = false;
                let pid = self
                    .discovery
                    .pop_front()
                    .expect("discovery queue is nonempty");
                if self.discover(pid) && self.enabled {
                    if tracked.contains(&pid)
                        && let Some(bundle) = self
                            .entries
                            .get(&pid)
                            .and_then(|entry| entry.reading.identity.bundle_path.clone())
                    {
                        bundles.insert(bundle);
                    }
                    if self.is_tracked(pid, &tracked, &bundles) {
                        samples.push_back((pid, false));
                    }
                }
            } else if let Some((pid, fair)) = samples.pop_front() {
                self.discovery_turn = true;
                self.priority_turn = fair;
                if fair {
                    self.cursor = pid;
                }
                self.sample(pid);
            }
        }
        self.snapshot(&tracked)
    }
    fn discover(&mut self, pid: u32) -> bool {
        if let Some(entry) = self.entries.get(&pid) {
            match self.system.matches(&entry.reading.identity) {
                Ok(true) => {
                    self.entries.get_mut(&pid).expect("entry exists").verified = self.system.now();
                    return false;
                }
                Ok(false) => {
                    self.entries.remove(&pid);
                }
                Err(_) => {
                    self.entries.get_mut(&pid).expect("entry exists").fail();
                    return false;
                }
            }
        }
        if self.entries.len() >= PROCESS_LIMIT {
            self.limited = true;
            return false;
        }
        match self.system.capture(pid) {
            Ok(identity) => {
                self.entries
                    .insert(pid, Entry::new(identity, self.system.now()));
                true
            }
            Err(_) => {
                self.capture_failures += 1;
                false
            }
        }
    }
    fn sample_order(&self, interest: &ResourceInterest, now: Duration) -> VecDeque<(u32, bool)> {
        let tracked: BTreeSet<_> = interest
            .tracked_pids
            .iter()
            .copied()
            .take(PROCESS_LIMIT)
            .collect();
        let tracked_bundles = self.bundles_for(&tracked);
        let due = |pid: &u32| {
            self.is_tracked(*pid, &tracked, &tracked_bundles)
                && self.entries.get(pid).is_some_and(|entry| {
                    entry
                        .attempted
                        .is_none_or(|last| now.saturating_sub(last) >= SAMPLE_INTERVAL)
                })
        };
        let prioritized: BTreeSet<_> = interest
            .priority_pids
            .iter()
            .copied()
            .take(PRIORITY_LIMIT)
            .collect();
        let prioritized_bundles = self.bundles_for(&prioritized);
        // Preserve selected/visible representative order before adding helpers.
        let mut priority = VecDeque::new();
        let mut boosted = BTreeSet::new();
        for pid in interest.priority_pids.iter().copied().take(PRIORITY_LIMIT) {
            if due(&pid) && boosted.insert(pid) {
                priority.push_back(pid);
            }
        }
        for pid in self.entries.keys().copied() {
            if priority.len() >= PRIORITY_LIMIT {
                break;
            }
            if self.is_tracked(pid, &prioritized, &prioritized_bundles)
                && due(&pid)
                && boosted.insert(pid)
            {
                priority.push_back(pid);
            }
        }
        let mut normal: VecDeque<_> = self
            .entries
            .range((
                std::ops::Bound::Excluded(self.cursor),
                std::ops::Bound::Unbounded,
            ))
            .chain(self.entries.range(..=self.cursor))
            .map(|(pid, _)| *pid)
            .filter(due)
            .collect();
        let mut order = VecDeque::new();
        let mut emitted = BTreeSet::new();
        let mut boost = self.priority_turn;
        while !priority.is_empty() || !normal.is_empty() {
            // Priority gets a bounded boost, while every PID remains in the
            // fair rotation. Keep the first-choice phase across observations:
            // one slow reading can consume a whole call's cooperative budget.
            let (pid, fair) = if boost && !priority.is_empty() || normal.is_empty() {
                (
                    priority.pop_front().expect("priority queue is nonempty"),
                    false,
                )
            } else {
                (
                    normal.pop_front().expect("rotation queue is nonempty"),
                    true,
                )
            };
            if emitted.insert(pid) {
                order.push_back((pid, fair));
            }
            boost = !boost;
        }
        order
    }
    fn bundles_for(&self, representatives: &BTreeSet<u32>) -> BTreeSet<String> {
        representatives
            .iter()
            .filter_map(|pid| self.entries.get(pid)?.reading.identity.bundle_path.clone())
            .collect()
    }
    fn is_tracked(
        &self,
        pid: u32,
        representatives: &BTreeSet<u32>,
        bundles: &BTreeSet<String>,
    ) -> bool {
        if representatives.contains(&pid) {
            return true;
        }
        let Some(bundle) = self
            .entries
            .get(&pid)
            .and_then(|entry| entry.reading.identity.bundle_path.as_ref())
        else {
            return false;
        };
        bundles.contains(bundle)
    }
    fn sample(&mut self, pid: u32) {
        let Some(entry) = self.entries.get_mut(&pid) else {
            return;
        };
        let now = self.system.now();
        if entry
            .attempted
            .is_some_and(|last| now.saturating_sub(last) < SAMPLE_INTERVAL)
        {
            return;
        }
        entry.attempted = Some(now);
        let identity = entry.reading.identity.clone();
        match self.system.matches(&identity) {
            Ok(false) => {
                self.entries.remove(&pid);
                self.queue_discovery(pid);
                return;
            }
            Err(_) => {
                self.entries.get_mut(&pid).expect("entry exists").fail();
                return;
            }
            Ok(true) => {}
        }
        let counters = self.system.counters(pid);
        // Bracket counters with generation checks to reject exec/reuse races.
        match self.system.matches(&identity) {
            Ok(false) => {
                self.entries.remove(&pid);
                self.queue_discovery(pid);
            }
            Err(_) => self.entries.get_mut(&pid).expect("entry exists").fail(),
            Ok(true) => {
                let entry = self.entries.get_mut(&pid).expect("entry exists");
                entry.verified = self.system.now();
                match counters {
                    Ok(counters) => entry.update(self.system.now(), counters),
                    Err(_) => entry.fail(),
                }
            }
        }
    }
    fn queue_discovery(&mut self, pid: u32) {
        if !self.discovery.contains(&pid) {
            if self.discovery.len() < PROCESS_LIMIT {
                self.discovery.push_back(pid);
            } else {
                self.limited = true;
            }
        }
    }
    fn snapshot(&self, tracked: &BTreeSet<u32>) -> Resources {
        let mut resources = Resources {
            enabled: self.enabled,
            message: self.message.clone(),
            ..Resources::default()
        };
        if !self.enabled {
            return resources;
        }
        let now = self.system.now();
        let bundles = self.bundles_for(tracked);
        let mut grouped: BTreeMap<String, Vec<ProcessReading>> = BTreeMap::new();
        for entry in self.entries.values() {
            let identity = &entry.reading.identity;
            if !self.is_tracked(identity.pid, tracked, &bundles) {
                continue;
            }
            let key = identity
                .bundle_path
                .clone()
                .unwrap_or_else(|| format!("pid:{}", identity.pid));
            grouped.entry(key).or_default().push(entry.reading(now));
        }
        resources.groups = grouped
            .into_iter()
            .map(|(key, processes)| (key, aggregate(processes)))
            .collect();
        let pending = !self.discovery.is_empty();
        if resources.message.is_none() && (self.limited || pending || self.capture_failures > 0) {
            resources.message = Some(if self.limited {
                "Process discovery reached its 4096-process limit; coverage is partial".into()
            } else if pending {
                "Process discovery is continuing within its work budget; coverage is partial".into()
            } else {
                "Some process identities could not be captured; coverage is partial".into()
            });
        }
        // Unknown or unverified members might belong to any observed bundle.
        // Do not describe a currently known subset as complete coverage.
        if resources.message.is_some() {
            for usage in resources.groups.values_mut() {
                if matches!(usage.state, ReadingState::Fresh | ReadingState::WarmingUp) {
                    usage.state = ReadingState::Partial;
                }
            }
        }
        resources
    }
}
fn aggregate(processes: Vec<ProcessReading>) -> Usage {
    let state = processes
        .first()
        .map_or(ReadingState::Unavailable, |first| {
            if processes.iter().all(|process| process.state == first.state) {
                first.state
            } else {
                ReadingState::Partial
            }
        });
    Usage {
        process_count: processes.len(),
        sampled_count: processes
            .iter()
            .filter(|process| {
                matches!(process.state, ReadingState::Fresh | ReadingState::WarmingUp)
            })
            .count(),
        cpu_percent: sum_cpu(processes.iter().filter_map(|process| process.cpu_percent)),
        memory_bytes: sum_bytes(processes.iter().filter_map(|process| process.memory_bytes)),
        read_per_sec: sum_bytes(processes.iter().filter_map(|process| process.read_per_sec)),
        write_per_sec: sum_bytes(processes.iter().filter_map(|process| process.write_per_sec)),
        state,
        age_ms: processes.iter().filter_map(|process| process.age_ms).max(),
        processes,
    }
}
fn sum_bytes(mut values: impl Iterator<Item = u64>) -> Option<u64> {
    let first = values.next()?;
    Some(values.fold(first, u64::saturating_add))
}
fn sum_cpu(mut values: impl Iterator<Item = f64>) -> Option<f64> {
    let first = values.next()?;
    let total = values.fold(first, |sum, value| sum + value);
    total.is_finite().then_some(total)
}
