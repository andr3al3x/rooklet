//! Shared profile validation, review, managed storage, and verified application.
mod preparation;
mod review;
mod store;
mod transaction;

use crate::{
    backend::Backend,
    model::{Profile, Snapshot},
};
use anyhow::Result;
pub use preparation::Prepared;
use std::path::Path;

/// Read a bounded, regular JSON file without following a final symlink.
pub fn read(path: &Path) -> Result<Profile> {
    preparation::read(path)
}
/// Pure schema, path syntax, size, and rule validation; no live queries.
pub fn validate(profile: &Profile) -> Result<()> {
    preparation::validate(profile)
}
pub fn export(snapshot: &Snapshot) -> Result<Profile> {
    preparation::export(snapshot)
}
/// Resolve application registrations and validate the complete proposed change.
pub fn prepare(profile: &Profile, snapshot: &Snapshot) -> Result<Prepared> {
    preparation::prepare(profile, snapshot)
}
/// Reject changed review baselines, apply all scopes, and verify final readback.
pub fn apply(backend: &mut Backend, prepared: &Prepared) -> Result<()> {
    transaction::apply(backend, prepared)
}
pub fn list() -> Result<Vec<String>> {
    store::list()
}
pub fn load(name: &str) -> Result<Profile> {
    store::load(name)
}
/// Save privately and atomically; existing names are never overwritten.
pub fn save(name: &str, profile: &Profile) -> Result<()> {
    store::save(name, profile)
}
