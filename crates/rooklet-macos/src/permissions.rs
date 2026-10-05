//! Capture filesystem evidence for pure incoming permission resolution.
use crate::process;
use rooklet_core::{model::Snapshot, permissions::Paths};
use std::path::Path;

/// Capture canonical paths and verified bundle membership outside rendering/input.
pub fn capture_paths(snapshot: &Snapshot) -> Paths {
    let mut paths = Paths::default();
    for application in &snapshot.applications {
        let canonical = canonical_key(&application.path);
        let bundle = registered_bundle(Path::new(&canonical));
        paths.record_registration(application.path.clone(), canonical, bundle);
    }
    for process in &snapshot.activity {
        if let Some(path) = &process.path {
            paths.record_activity(path.clone(), canonical_key(path));
        }
    }
    paths
}

fn canonical_key(path: &str) -> String {
    Path::new(path)
        .canonicalize()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_owned())
}
fn registered_bundle(path: &Path) -> Option<String> {
    let bundle = if path.is_dir()
        && path.extension().is_some_and(|extension| extension == "app")
        && path.join("Contents/Info.plist").is_file()
    {
        process::verified_bundle(&path.join("Contents/Info.plist"))?
    } else {
        process::verified_bundle(path)?
    };
    Some(bundle.to_string_lossy().into_owned())
}
