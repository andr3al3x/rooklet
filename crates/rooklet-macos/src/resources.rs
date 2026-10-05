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
        self.engine.observe(interest, cancel)
    }
    pub(crate) fn identities(&self) -> Vec<ProcessIdentity> {
        self.engine.identities()
    }
}
