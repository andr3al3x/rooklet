//! Identity-checked process signaling. Signal delivery does not prove process exit.
mod engine;
pub(crate) mod native;
#[cfg(test)]
mod tests;

use anyhow::Result;
use rooklet_core::process::{ProcessIdentity, TerminationReport, TerminationRequest};
use std::path::{Path, PathBuf};

#[cfg(test)]
pub(crate) const MAX_PROCESSES: usize = 16_384;
pub(crate) fn capture(pid: u32) -> Result<ProcessIdentity> {
    native::capture(pid)
}
pub(crate) fn process_path(pid: u32) -> Option<String> {
    native::process_path(pid)
}
/// Bound enumeration to the current effective user, including processes with no traffic.
#[cfg(test)]
pub(crate) fn capture_snapshot() -> Result<Vec<ProcessIdentity>> {
    engine::capture_snapshot(&mut native::Native)
}
/// Shared application grouping: the outermost on-disk bundle with an Info.plist.
pub(crate) fn verified_bundle(path: &Path) -> Option<PathBuf> {
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
pub(crate) fn terminate(request: &TerminationRequest) -> Result<TerminationReport> {
    engine::terminate(&mut native::Native, request)
}
