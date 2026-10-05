//! Resource observations and collection interest; no native collection.
use crate::{model::ProcessActivity, process::ProcessIdentity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Default)]
pub struct ResourceInterest {
    pub enabled: bool,
    pub tracked_pids: Vec<u32>,
    pub priority_pids: Vec<u32>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Resources {
    pub enabled: bool,
    pub groups: BTreeMap<String, Usage>,
    pub message: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadingState {
    Fresh,
    WarmingUp,
    Stale,
    Unavailable,
    Partial,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub process_count: usize,
    pub sampled_count: usize,
    pub cpu_percent: Option<f64>,
    pub memory_bytes: Option<u64>,
    pub read_per_sec: Option<u64>,
    pub write_per_sec: Option<u64>,
    pub state: ReadingState,
    pub age_ms: Option<u64>,
    pub processes: Vec<ProcessReading>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessReading {
    pub identity: ProcessIdentity,
    pub cpu_percent: Option<f64>,
    pub memory_bytes: Option<u64>,
    pub read_per_sec: Option<u64>,
    pub write_per_sec: Option<u64>,
    pub state: ReadingState,
    pub age_ms: Option<u64>,
}
impl Resources {
    pub fn for_activity(&self, activity: &ProcessActivity) -> Option<&Usage> {
        // Activity rows may outlive an exec or PID reuse, even with the same path.
        let identity = activity
            .identities
            .iter()
            .find(|identity| identity.pid == activity.pid)
            .or_else(|| {
                activity
                    .identities
                    .first()
                    .filter(|identity| identity.bundle_path.is_some())
            })?;
        let key = identity
            .bundle_path
            .clone()
            .unwrap_or_else(|| format!("pid:{}", identity.pid));
        let usage = self.groups.get(&key)?;
        usage
            .processes
            .iter()
            .any(|reading| &reading.identity == identity)
            .then_some(usage)
    }
}
