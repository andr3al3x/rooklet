//! Bounded, native, current-user resource observations. Readings are not enforcement verdicts.
mod engine;
mod native;
#[cfg(test)]
mod tests;

use rooklet_core::{
    process::ProcessIdentity,
    resources::{ResourceInterest, Resources},
};
use std::sync::atomic::AtomicBool;

#[derive(Default)]
pub(crate) struct Sampler {
    engine: engine::Engine<native::Native>,
}
impl Sampler {
    pub(crate) fn observe(
        &mut self,
        interest: &ResourceInterest,
        cancel: &AtomicBool,
    ) -> Resources {
        let started = std::time::Instant::now();
        let resources = self.engine.observe(interest, cancel);
        tracing::trace!(
            duration_us = started.elapsed().as_micros() as u64,
            enabled = resources.enabled,
            group_count = resources.groups.len(),
            "native resource observation completed"
        );
        resources
    }
    pub(crate) fn identities(&self) -> Vec<ProcessIdentity> {
        self.engine.identities()
    }
}
