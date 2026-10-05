//! Join registered ALF executable entries to verified Activity app bundles.
use crate::model::{ProcessActivity, Snapshot};
use std::collections::HashMap;

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
    /// Record a backend-verified registration, retaining its exact mutation target.
    pub fn record_registration(
        &mut self,
        path: String,
        canonical: String,
        verified_bundle: Option<String>,
    ) {
        if let Some(bundle) = verified_bundle {
            self.bundles.insert(path.clone(), bundle);
        } else {
            self.bundles.remove(&path);
        }
        self.canonical.insert(path, canonical);
    }
    /// Record the canonical identity of an observed Activity path.
    pub fn record_activity(&mut self, path: String, canonical: String) {
        self.activity.entry(path).or_insert(canonical);
    }
    fn key<'a>(&'a self, path: &'a str) -> &'a str {
        self.canonical.get(path).map(String::as_str).unwrap_or(path)
    }
    /// Freeze process identity evidence without freezing current ALF registrations.
    pub fn preserve_activity(&mut self, previous: &Self) {
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
