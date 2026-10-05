//! Captured process identities and explicit termination requests.
use serde::{Deserialize, Serialize};
/// Maximum confirmed targets; grouping retains one extra member to reject overflow.
pub const MAX_TARGETS: usize = 256;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub uid: u32,
    pub parent_pid: u32,
    pub start_sec: u64,
    pub start_usec: u64,
    /// Kernel PID generation, also changes across exec on macOS.
    pub pid_version: u32,
    pub path: String,
    pub bundle_path: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminationMode {
    Terminate,
    ForceKill,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminationRequest {
    pub targets: Vec<ProcessIdentity>,
    pub mode: TerminationMode,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalFailure {
    pub pid: u32,
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminationReport {
    /// Number of captured targets checked, including per-target races/errors.
    pub attempted: usize,
    pub delivered: Vec<u32>,
    pub failures: Vec<SignalFailure>,
}

/// Select exactly the captured app members; never discover new targets during termination.
pub fn members_for(snapshot: &[ProcessIdentity], pid: u32) -> Vec<ProcessIdentity> {
    let Some(selected) = snapshot.iter().find(|process| process.pid == pid) else {
        return Vec::new();
    };
    let mut members: Vec<_> = snapshot
        .iter()
        .filter(|process| {
            process.uid == selected.uid
                && match &selected.bundle_path {
                    Some(bundle) => process.bundle_path.as_ref() == Some(bundle),
                    None => process.pid == pid,
                }
        })
        .take(MAX_TARGETS + 1)
        .cloned()
        .collect();
    members.sort_by_key(|process| process.pid);
    members
}
