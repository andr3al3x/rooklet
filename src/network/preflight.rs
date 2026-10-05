//! Read-only host validation shared by profile preparation and PF application.
use super::{
    ANCHOR_FILE, CONFIG, MAX_FILE,
    compiler::compile_rules,
    config::{configured, has_parent_anchor, parent_matches_managed_config},
    persistence::{
        State, bounded_read, read_state, trusted, trusted_parents, validate_state_destination,
    },
    pf::{root, run, validate_pf, verify_interfaces},
};
use crate::model::NetworkRule;
use anyhow::{Result, ensure};
use std::path::Path;

pub(super) struct Validated {
    pub source: String,
    pub expected: String,
    pub previous: String,
}

/// Checks interfaces, trusted configuration, PF syntax, and persisted state size.
/// No rules, files, enable references, or connection states are changed.
pub fn preflight_apply(rules: &[NetworkRule]) -> Result<()> {
    root()?;
    validate(rules)?;
    validate_state_destination()?;
    read_state()?;
    ensure!(
        parent_matches_managed_config(&run(&["-sr"], None)?),
        "live PF parent configuration differs from Xield's managed layout"
    );
    Ok(())
}

pub(super) fn validate(rules: &[NetworkRule]) -> Result<Validated> {
    let compiled = compile_rules(rules)?;
    verify_interfaces(rules)?;
    trusted_parents(Path::new(CONFIG))?;
    trusted(Path::new(CONFIG), false)?;
    ensure!(
        configured(&bounded_read(Path::new(CONFIG))?),
        "Xield's parent anchor is not configured; run network setup first"
    );
    ensure!(
        has_parent_anchor(&run(&["-sr"], None)?),
        "Live parent PF rules do not reference Xield; run network setup after checking your PF configuration"
    );
    trusted_parents(Path::new(ANCHOR_FILE))?;
    trusted(Path::new(ANCHOR_FILE), false)?;
    let previous = bounded_read(Path::new(ANCHOR_FILE))?;
    ensure!(
        previous.starts_with("# Xield network rules:"),
        "Xield's anchor file was replaced; refusing to overwrite it"
    );
    let expected = validate_pf(&compiled)?;
    let candidate = State {
        rules: rules.to_vec(),
        enable_token: Some("0".repeat(32)),
        active: true,
        loaded_rules: Some(expected.clone()),
    };
    ensure!(
        serde_json::to_vec_pretty(&candidate)?.len() as u64 <= MAX_FILE,
        "Compiled network state exceeds the 1 MiB safety limit; use fewer rules"
    );
    Ok(Validated {
        source: compiled,
        expected,
        previous,
    })
}
