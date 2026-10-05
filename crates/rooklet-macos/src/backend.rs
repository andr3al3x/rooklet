//! macOS firewall operations and local activity observations.
mod alf;
mod cache;
mod live;
mod parser;

use anyhow::{Result, ensure};
use live::Live;
use rooklet_core::model::{Mutation, Snapshot};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
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
        self.live.snapshot(&self.cancel, true)
    }
    /// Routine observations reuse recent controls; refreshes and mutations force a read.
    pub fn observe(
        &mut self,
        interest: &rooklet_core::resources::ResourceInterest,
        force_controls: bool,
    ) -> Result<Snapshot> {
        ensure!(!self.cancel.load(Ordering::Relaxed), "snapshot cancelled");
        self.live.set_resource_interest(interest);
        self.live.snapshot(&self.cancel, force_controls)
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
        request: &rooklet_core::process::TerminationRequest,
    ) -> Result<rooklet_core::process::TerminationReport> {
        ensure!(
            !self.cancel.load(Ordering::Relaxed),
            "termination cancelled"
        );
        crate::process::terminate(request)
    }
    /// Validate the entire proposed PF configuration without changing live state.
    pub(crate) fn preflight_network_rules(
        &self,
        rules: &[rooklet_core::model::NetworkRule],
    ) -> Result<()> {
        ensure!(
            !self.cancel.load(Ordering::Relaxed),
            "profile validation cancelled"
        );
        crate::network::request_preflight(rules, &self.cancel)
    }

    pub fn mutate(&mut self, mutation: Mutation) -> Result<()> {
        ensure!(!self.cancel.load(Ordering::Relaxed), "mutation cancelled");
        let operation = match &mutation {
            Mutation::NetworkRules(_) => "network_rules",
            Mutation::Setting(_, _) => "incoming_setting",
            Mutation::Applications { .. } => "incoming_permissions",
            Mutation::AddApplication(_) => "add_application",
            Mutation::RemoveApplication(_) => "remove_application",
        };
        let _span = tracing::info_span!("backend_mutation", operation).entered();
        tracing::info!(outcome = "accepted", "firewall change accepted");
        let result = (|| {
            match mutation {
                Mutation::NetworkRules(rules) => {
                    crate::network::request_change(
                        crate::network::Change::Apply(&rules),
                        &self.cancel,
                    )?;
                }
                Mutation::Setting(setting, value) => {
                    alf::set_setting(setting, value, &self.cancel)?
                }
                Mutation::Applications { paths, action } => {
                    alf::set_applications(&paths, action, &self.cancel)?
                }
                Mutation::AddApplication(path) => alf::add_application(&path, &self.cancel)?,
                Mutation::RemoveApplication(path) => alf::remove_application(&path, &self.cancel)?,
            }
            Ok(())
        })();
        match &result {
            Ok(()) => tracing::info!(
                outcome = "verified",
                "firewall change completed with readback"
            ),
            Err(_) => tracing::error!(
                operation,
                outcome = "failed",
                "firewall change or readback failed"
            ),
        }
        result
    }
}

#[cfg(test)]
#[path = "backend/tests.rs"]
mod regression_tests;
