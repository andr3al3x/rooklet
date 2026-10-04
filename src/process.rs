//! Identity-checked process signaling. Signal delivery does not prove process exit.
mod engine;
mod native;
#[cfg(test)]
mod tests;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub(crate) const MAX_PROCESSES: usize = 16_384;
pub(crate) const MAX_TARGETS: usize = 256;
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

pub fn capture(pid: u32) -> Result<ProcessIdentity> {
    native::capture(pid)
}
pub fn process_path(pid: u32) -> Option<String> {
    native::process_path(pid)
}
/// Bound enumeration to the current effective user, including processes with no traffic.
pub fn capture_snapshot() -> Result<Vec<ProcessIdentity>> {
    engine::capture_snapshot(&mut native::Native)
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
/// Shared application grouping: the outermost on-disk bundle with an Info.plist.
pub fn verified_bundle(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() || !path.is_file() {
        return None;
    }
    path.ancestors()
        .filter(|candidate| {
            candidate
                .extension()
                .is_some_and(|extension| extension == "app")
                && candidate.is_dir()
                && candidate.join("Contents/Info.plist").is_file()
        })
        .last()
        .map(Path::to_path_buf)
}
/// Requires the caller's explicit confirmation for this mode and exact captured target list.
pub fn terminate(request: &TerminationRequest) -> Result<TerminationReport> {
    engine::terminate(&mut native::Native, request)
}
