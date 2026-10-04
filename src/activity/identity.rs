//! Resolve real executable paths and verify application bundle grouping.
use std::path::Path;

pub(super) fn group_identity(
    pid: u32,
    name: &str,
    path: Option<String>,
) -> (String, String, Option<String>) {
    if let Some(executable) = &path
        && let Some(bundle) = crate::process::verified_bundle(Path::new(executable))
    {
        let label = bundle
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(name)
            .to_owned();
        return (
            bundle.to_string_lossy().into_owned(),
            label,
            Some(bundle.to_string_lossy().into_owned()),
        );
    }
    (format!("pid:{pid}"), name.into(), path)
}
pub(super) fn process_path(pid: u32) -> Option<String> {
    crate::process::process_path(pid)
}
