//! Narrow incoming requests; privileged helpers own tool deadlines and cleanup.
use super::alf;
use crate::command;
use anyhow::{Context, Result, bail, ensure};
use rooklet_core::{
    application::validate_path,
    model::{Action, Mutation, Setting},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_REQUEST: usize = 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Change {
    Setting { setting: Setting, value: bool },
    Applications { paths: Vec<String>, action: Action },
    Add { path: String },
    Remove { path: String },
}

impl Change {
    fn from_mutation(mutation: Mutation) -> Result<Self> {
        Ok(match mutation {
            Mutation::Setting(setting, value) => Self::Setting { setting, value },
            Mutation::Applications { paths, action } => Self::Applications { paths, action },
            Mutation::AddApplication(path) => Self::Add { path },
            Mutation::RemoveApplication(path) => Self::Remove { path },
            Mutation::NetworkRules(_) => bail!("incoming helper cannot change PF rules"),
        })
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Setting { .. } => Ok(()),
            Self::Applications { paths, .. } => {
                ensure!(
                    !paths.is_empty() && paths.len() <= 256,
                    "application action requires between 1 and 256 registered paths"
                );
                let mut unique = HashSet::new();
                for path in paths {
                    validate_path(path)?;
                    ensure!(unique.insert(path), "duplicate application target {path:?}");
                }
                Ok(())
            }
            Self::Add { path } | Self::Remove { path } => validate_path(path),
        }
    }

    fn execute(self) -> Result<()> {
        // The accepted operation runs to completion/readback. This process is
        // root, so its bounded command owner can terminate and reap its tools.
        let cancel = AtomicBool::new(false);
        match self {
            Self::Setting { setting, value } => alf::set_setting(setting, value, &cancel),
            Self::Applications { paths, action } => alf::set_applications(&paths, action, &cancel),
            Self::Add { path } => alf::add_application(&path, &cancel),
            Self::Remove { path } => alf::remove_application(&path, &cancel),
        }
    }
}

pub(super) fn request(mutation: Mutation, cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "incoming change cancelled");
    let change = Change::from_mutation(mutation)?;
    change.validate()?;
    if command::is_root() {
        return change.execute();
    }
    let input = serde_json::to_vec(&change)?;
    ensure!(input.len() <= MAX_REQUEST, "incoming request exceeds 1 MiB");
    command::run_transaction(
        &command::self_executable()?,
        &["__incoming".into()],
        Some(&input),
        true,
        cancel,
    )?;
    Ok(())
}

pub(super) fn finish(input: &[u8]) -> Result<()> {
    ensure!(
        cfg!(target_os = "macos"),
        "incoming firewall changes require macOS"
    );
    finish_with(input, command::is_root(), Change::execute)
}

fn finish_with(input: &[u8], root: bool, execute: impl FnOnce(Change) -> Result<()>) -> Result<()> {
    ensure!(root, "incoming helper requires root");
    ensure!(input.len() <= MAX_REQUEST, "incoming request exceeds 1 MiB");
    let change: Change =
        serde_json::from_slice(input).context("invalid incoming helper request")?;
    change.validate()?;
    execute(change)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTING: &[u8] = br#"{"operation":"setting","setting":"stealth","value":true}"#;

    #[test]
    fn helper_rejects_non_root_before_dispatch_or_decoding() {
        let error =
            finish_with(b"not JSON", false, |_| panic!("dispatched without root")).unwrap_err();
        assert!(error.to_string().contains("requires root"));
    }

    #[test]
    fn helper_rejects_unsupported_requests_without_execution() {
        let mut oversized = vec![b' '; MAX_REQUEST + 1];
        oversized[..SETTING.len()].copy_from_slice(SETTING);
        for input in [
            b"not JSON".as_slice(),
            br#"{"operation":"network_rules","rules":[]}"#,
            br#"{"operation":"setting","setting":"stealth","value":true,"program":"/bin/sh"}"#,
            br#"{"operation":"setting","setting":"unknown","value":true}"#,
            br#"{"operation":"remove","path":"relative"}"#,
            br#"{"operation":"applications","paths":[],"action":"block"}"#,
            br#"{"operation":"applications","paths":["/app","/app"],"action":"block"}"#,
            oversized.as_slice(),
        ] {
            assert!(finish_with(input, true, |_| panic!("executed invalid input")).is_err());
        }
        let too_many = serde_json::to_vec(&Change::Applications {
            paths: (0..257).map(|index| format!("/app{index}")).collect(),
            action: Action::Block,
        })
        .unwrap();
        assert!(finish_with(&too_many, true, |_| panic!("executed too many targets")).is_err());
    }

    #[test]
    fn accepted_request_preserves_typed_target_and_reports_operation_failure() {
        let error = finish_with(SETTING, true, |change| {
            assert!(matches!(
                change,
                Change::Setting {
                    setting: Setting::Stealth,
                    value: true
                }
            ));
            bail!("readback unavailable; setting may have changed")
        })
        .unwrap_err();
        assert!(error.to_string().contains("setting may have changed"));
        let input = serde_json::to_vec(&Change::Remove {
            path: "/app with trailing space ".into(),
        })
        .unwrap();
        finish_with(&input, true, |change| {
            assert!(
                matches!(change, Change::Remove { path } if path == "/app with trailing space ")
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn cancelled_and_invalid_parent_requests_never_launch_a_helper() {
        assert!(
            request(
                Mutation::Setting(Setting::Stealth, true),
                &AtomicBool::new(true)
            )
            .is_err()
        );
        for mutation in [
            Mutation::AddApplication("relative".into()),
            Mutation::RemoveApplication("/../app".into()),
            Mutation::Applications {
                paths: Vec::new(),
                action: Action::Block,
            },
            Mutation::NetworkRules(Vec::new()),
        ] {
            assert!(request(mutation, &AtomicBool::new(false)).is_err());
        }
    }
}
