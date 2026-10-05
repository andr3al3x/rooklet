//! macOS firewall operations and local activity observations.
mod alf;
mod application;
mod live;
mod parser;

pub use application::{registration_path, validate_application_path};
pub use parser::{parse_app_blocked, parse_applications, parse_settings};

use crate::{
    command,
    model::{Mutation, Snapshot},
};
use anyhow::{Context, Result, ensure};
use live::Live;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub struct Backend {
    live: Live,
    cancel: Arc<AtomicBool>,
}
impl Backend {
    pub fn new() -> Result<Self> {
        Ok(Self {
            live: Live::new()?,
            cancel: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn with_cancellation(mut self, cancel: Arc<AtomicBool>) -> Self {
        self.cancel = cancel;
        self
    }
    pub fn snapshot(&mut self) -> Result<Snapshot> {
        ensure!(!self.cancel.load(Ordering::Relaxed), "snapshot cancelled");
        self.live.snapshot(&self.cancel)
    }
    pub fn update_geoip(&mut self) -> Result<()> {
        ensure!(
            !self.cancel.load(Ordering::Relaxed),
            "country database update cancelled"
        );
        crate::geoip::update(&self.cancel)?;
        self.live.reload_geoip()
    }
    pub fn terminate(
        &mut self,
        request: &crate::process::TerminationRequest,
    ) -> Result<crate::process::TerminationReport> {
        ensure!(
            !self.cancel.load(Ordering::Relaxed),
            "termination cancelled"
        );
        crate::process::terminate(request)
    }
    /// Validate the entire proposed PF configuration without changing live state.
    pub fn preflight_network_rules(&self, rules: &[crate::model::NetworkRule]) -> Result<()> {
        ensure!(
            !self.cancel.load(Ordering::Relaxed),
            "profile validation cancelled"
        );
        crate::network::validate_rules(rules)?;
        let executable = std::env::current_exe()
            .context("unable to locate xield executable")?
            .canonicalize()?;
        command::run_with_timeout(
            &executable,
            &["network".into(), "preflight".into(), "--stdin".into()],
            Some(&serde_json::to_vec(rules)?),
            true,
            &self.cancel,
            Duration::from_secs(90),
        )?;
        Ok(())
    }

    pub fn mutate(&mut self, mutation: Mutation) -> Result<()> {
        ensure!(!self.cancel.load(Ordering::Relaxed), "mutation cancelled");
        match mutation {
            Mutation::NetworkRules(rules) => {
                crate::network::validate_rules(&rules)?;
                let executable = std::env::current_exe()
                    .context("unable to locate xield executable")?
                    .canonicalize()?;
                let args = vec!["network".into(), "apply".into(), "--stdin".into()];
                command::run_with_timeout(
                    &executable,
                    &args,
                    Some(&serde_json::to_vec(&rules)?),
                    true,
                    &self.cancel,
                    Duration::from_secs(90),
                )?;
            }
            Mutation::Setting(setting, value) => alf::set_setting(setting, value, &self.cancel)?,
            Mutation::Applications { paths, action } => {
                alf::set_applications(&paths, action, &self.cancel)?
            }
            Mutation::AddApplication(path) => alf::add_application(&path, &self.cancel)?,
            Mutation::RemoveApplication(path) => alf::remove_application(&path, &self.cancel)?,
        }
        Ok(())
    }
}
