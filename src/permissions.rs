//! Join registered ALF executable entries to verified Activity app bundles.
use crate::{
    model::{ProcessActivity, Snapshot},
    process,
};
use std::{collections::HashMap, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncomingState {
    Allow,
    Block,
    Mixed,
    Unlisted,
    Unknown,
}
impl IncomingState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::Block => "Block",
            Self::Mixed => "Mixed",
            Self::Unlisted => "Unlisted",
            Self::Unknown => "Unknown",
        }
    }
}
pub struct Resolution {
    pub state: IncomingState,
    /// Exact captured registration paths, never the displayed bundle label.
    pub paths: Vec<String>,
}
#[derive(Default)]
pub struct Index {
    available: bool,
    entries: HashMap<String, bool>,
    bundles: HashMap<String, Vec<String>>,
    executables: HashMap<String, Vec<String>>,
    canonical: HashMap<String, String>,
}
/// Filesystem evidence captured by the backend, never during rendering or input.
#[derive(Debug, Clone, Default)]
pub struct Paths {
    canonical: HashMap<String, String>,
    activity: HashMap<String, String>,
    bundles: HashMap<String, String>,
}
impl Paths {
    pub fn capture(snapshot: &Snapshot) -> Self {
        let mut paths = Self::default();
        for application in &snapshot.applications {
            let canonical = canonical_key(&application.path);
            if let Some(bundle) = registered_bundle(Path::new(&canonical)) {
                paths.bundles.insert(application.path.clone(), bundle);
            }
            paths.canonical.insert(application.path.clone(), canonical);
        }
        for process in &snapshot.activity {
            if let Some(path) = &process.path {
                paths
                    .activity
                    .entry(path.clone())
                    .or_insert_with(|| canonical_key(path));
            }
        }
        paths
    }
    fn key<'a>(&'a self, path: &'a str) -> &'a str {
        self.canonical.get(path).map(String::as_str).unwrap_or(path)
    }
    /// Freeze process identity evidence without freezing current ALF registrations.
    pub(crate) fn preserve_activity(&mut self, previous: &Self) {
        self.activity = previous.activity.clone();
    }
    fn activity_key<'a>(&'a self, path: &'a str) -> &'a str {
        self.activity.get(path).map(String::as_str).unwrap_or(path)
    }
}
impl Index {
    /// Build a pure lookup from captured paths and process identities.
    pub fn new(snapshot: &Snapshot) -> Self {
        let captured: HashMap<_, _> = snapshot
            .activity
            .iter()
            .flat_map(|p| &p.identities)
            .filter_map(|identity| {
                identity
                    .bundle_path
                    .as_deref()
                    .map(|bundle| (identity.path.as_str(), bundle))
            })
            .collect();
        let mut index = Self {
            available: snapshot.applications_available,
            canonical: snapshot.permission_paths.activity.clone(),
            ..Self::default()
        };
        for application in &snapshot.applications {
            index
                .executables
                .entry(snapshot.permission_paths.key(&application.path).to_owned())
                .or_default()
                .push(application.path.clone());
            index
                .entries
                .insert(application.path.clone(), application.blocked);
            if let Some(bundle) = snapshot
                .permission_paths
                .bundles
                .get(&application.path)
                .cloned()
                .or_else(|| {
                    captured
                        .get(application.path.as_str())
                        .map(|path| snapshot.permission_paths.activity_key(path).to_owned())
                })
            {
                index
                    .bundles
                    .entry(bundle)
                    .or_default()
                    .push(application.path.clone());
            }
        }
        for entries in index
            .bundles
            .values_mut()
            .chain(index.executables.values_mut())
        {
            entries.sort();
            entries.dedup();
        }
        index
    }
    pub fn activity(&self, process: &ProcessActivity) -> Resolution {
        let Some(path) = process.path.as_deref().filter(|_| self.available) else {
            return Resolution {
                state: IncomingState::Unknown,
                paths: Vec::new(),
            };
        };
        let key = self.canonical.get(path).map(String::as_str).unwrap_or(path);
        let paths = self
            .bundles
            .get(key)
            .or_else(|| self.executables.get(key))
            .cloned()
            .unwrap_or_default();
        let blocked = paths
            .iter()
            .filter(|path| self.entries.get(*path) == Some(&true))
            .count();
        let state = match (paths.len(), blocked) {
            (0, _) => IncomingState::Unlisted,
            (_, 0) => IncomingState::Allow,
            (count, blocked) if count == blocked => IncomingState::Block,
            _ => IncomingState::Mixed,
        };
        Resolution { state, paths }
    }
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
