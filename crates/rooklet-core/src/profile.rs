//! Strict profile parsing, validation, and export without system access.
use crate::{
    application,
    model::{Profile, Snapshot},
    network,
};
use anyhow::{Context, Result, ensure};
use std::collections::HashSet;

pub const MAX_BYTES: usize = 1024 * 1024;

/// Parse bounded JSON input and validate all scopes before returning a profile.
pub fn parse(bytes: &[u8]) -> Result<Profile> {
    ensure!(bytes.len() <= MAX_BYTES, "configuration exceeds 1 MiB");
    let profile = serde_json::from_slice(bytes).context("invalid Rooklet profile")?;
    validate(&profile)?;
    Ok(profile)
}

pub fn validate(profile: &Profile) -> Result<()> {
    ensure!(
        profile.format == "rooklet-profile" && profile.version == 1,
        "unsupported profile format"
    );
    ensure!(
        profile.firewall.is_some(),
        "profile must contain firewall settings"
    );
    network::validate_rules(&profile.network_rules)?;
    ensure!(
        profile.applications.len() <= 4096,
        "too many application entries"
    );
    let mut paths = HashSet::new();
    for app in &profile.applications {
        application::validate_path(&app.path)?;
        ensure!(
            app.name.len() <= 4096,
            "application name exceeds 4096 bytes"
        );
        ensure!(paths.insert(&app.path), "duplicate application path");
    }
    ensure!(
        serde_json::to_vec(profile)?.len() <= MAX_BYTES,
        "configuration exceeds 1 MiB"
    );
    Ok(())
}
pub fn export(snapshot: &Snapshot) -> Result<Profile> {
    ensure!(
        snapshot.firewall.is_some(),
        "cannot export unavailable firewall settings"
    );
    ensure!(
        snapshot.applications_available,
        "cannot export unavailable incoming application entries"
    );
    ensure!(
        snapshot.network.rules_available,
        "saved network rules are unavailable; authenticate before using profiles"
    );
    let profile = Profile::from_snapshot(snapshot);
    validate(&profile)?;
    Ok(profile)
}
