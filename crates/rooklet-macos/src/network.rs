//! Typed, bounded requests for macOS PF operations in Rooklet's managed anchor.
mod config;
mod lifecycle;
mod persistence;
mod pf;
mod preflight;

use crate::command;
use anyhow::{Context, Result, ensure};
use rooklet_core::{
    model::{NetworkRule, NetworkStatus},
    network::validate_rules,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

const CONFIG: &str = "/etc/pf.conf";
const ANCHOR_FILE: &str = "/etc/pf.anchors/rooklet";
const MAX_FILE: u64 = 1_048_576;

/// Complete PF changes accepted by the privileged helper.
pub enum Change<'a> {
    Setup(&'a [NetworkRule]),
    Apply(&'a [NetworkRule]),
    Disable,
    Remove,
}

fn executable() -> Result<PathBuf> {
    std::env::current_exe()
        .context("unable to locate rooklet executable")?
        .canonicalize()
        .context("unable to resolve rooklet executable")
}

/// Observe PF state without prompting for credentials.
pub fn request_status(cancel: &AtomicBool) -> Result<NetworkStatus> {
    ensure!(!cancel.load(Ordering::Relaxed), "network status cancelled");
    if command::is_root() {
        return Ok(lifecycle::status());
    }
    let result = command::run(
        &executable()?,
        &["network".into(), "status".into()],
        None,
        true,
        cancel,
    );
    match result {
        Ok(text) => serde_json::from_str(&text).context("invalid network status response"),
        Err(_) => Ok(NetworkStatus {
            message: Some(
                "PF status requires administrator access; run sudo -v or sudo rooklet network status".into(),
            ),
            ..Default::default()
        }),
    }
}

/// Finish an authorized transaction, including any restoration, once launched.
/// Authentication must already have completed outside terminal raw mode.
pub fn request_change(change: Change<'_>, cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "network change cancelled");
    let (action, rules) = match &change {
        Change::Setup(rules) => ("setup", Some(*rules)),
        Change::Apply(rules) => ("apply", Some(*rules)),
        Change::Disable => ("disable", None),
        Change::Remove => ("remove", None),
    };
    let _span = tracing::info_span!("pf_change", operation = action).entered();
    tracing::debug!(phase = "validation", "validating PF change request");
    if let Some(rules) = rules {
        validate_rules(rules)?;
    }
    tracing::info!(outcome = "accepted", "PF change accepted");
    let result = (|| {
        if command::is_root() {
            return match change {
                Change::Setup(rules) => lifecycle::setup(rules),
                Change::Apply(rules) => lifecycle::apply(rules),
                Change::Disable => lifecycle::disable(),
                Change::Remove => lifecycle::remove(),
            };
        }
        let mut args = vec!["network".into(), action.into()];
        let input = rules.map(serde_json::to_vec).transpose()?;
        if input.is_some() {
            args.push("--stdin".into());
        }
        command::run_transaction(&executable()?, &args, input.as_deref(), true, cancel)?;
        Ok(())
    })();
    match &result {
        Ok(()) => tracing::info!(outcome = "completed", "PF change completed"),
        Err(_) => tracing::error!(operation = action, outcome = "failed", "PF change failed"),
    }
    result
}

/// Validate the proposed PF configuration against this host without mutation.
/// Authentication must already have completed; this helper never prompts.
pub fn request_preflight(rules: &[NetworkRule], cancel: &AtomicBool) -> Result<()> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "network preflight cancelled"
    );
    let _span = tracing::debug_span!("pf_preflight", rule_count = rules.len()).entered();
    validate_rules(rules)?;
    if command::is_root() {
        return preflight::preflight_apply(rules);
    }
    command::run_with_timeout(
        &executable()?,
        &["network".into(), "preflight".into(), "--stdin".into()],
        Some(&serde_json::to_vec(rules)?),
        true,
        cancel,
        Duration::from_secs(90),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_requests_do_not_launch_or_mutate() {
        let cancel = AtomicBool::new(true);
        assert!(request_status(&cancel).is_err());
        assert!(request_preflight(&[], &cancel).is_err());
        for change in [
            Change::Setup(&[]),
            Change::Apply(&[]),
            Change::Disable,
            Change::Remove,
        ] {
            assert!(request_change(change, &cancel).is_err());
        }
    }

    #[test]
    fn invalid_rules_fail_before_privileged_operations() {
        let cancel = AtomicBool::new(false);
        let invalid = NetworkRule {
            id: String::new(),
            name: "invalid".into(),
            action: rooklet_core::model::Action::Block,
            destination: "any".into(),
            port: None,
            protocol: rooklet_core::model::Protocol::Any,
            direction: rooklet_core::model::Direction::Both,
            interface: None,
            enabled: true,
        };
        assert!(request_change(Change::Setup(std::slice::from_ref(&invalid)), &cancel).is_err());
        assert!(request_change(Change::Apply(std::slice::from_ref(&invalid)), &cancel).is_err());
        assert!(request_preflight(&[invalid], &cancel).is_err());
    }
}
