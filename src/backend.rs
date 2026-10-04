//! Firewall backend facade with explicit live and simulated implementations.
mod alf;
mod application;
mod demo;
mod live;
mod parser;

pub use application::{registration_path, validate_application_path};
pub use parser::{parse_app_blocked, parse_applications, parse_settings};

use crate::{
    command,
    model::{Mutation, Snapshot},
};
use anyhow::{Context, Result, ensure};
use demo::Demo;
use live::Live;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub struct Backend {
    state: State,
    cancel: Arc<AtomicBool>,
}
enum State {
    Demo(Box<Demo>),
    Live(Box<Live>),
}
impl Backend {
    pub fn new(demo: bool) -> Result<Self> {
        let state = if demo {
            State::Demo(Box::new(Demo::new()))
        } else {
            State::Live(Box::new(Live::new()?))
        };
        Ok(Self {
            state,
            cancel: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn with_cancellation(mut self, cancel: Arc<AtomicBool>) -> Self {
        self.cancel = cancel;
        self
    }
    pub fn is_demo(&self) -> bool {
        matches!(self.state, State::Demo(_))
    }
    pub fn snapshot(&mut self) -> Result<Snapshot> {
        ensure!(!self.cancel.load(Ordering::Relaxed), "snapshot cancelled");
        match &mut self.state {
            State::Demo(demo) => Ok(demo.snapshot()),
            State::Live(live) => live.snapshot(&self.cancel),
        }
    }
    pub fn update_geoip(&mut self) -> Result<()> {
        ensure!(
            !self.cancel.load(Ordering::Relaxed),
            "country database update cancelled"
        );
        match &mut self.state {
            State::Demo(_) => Ok(()),
            State::Live(live) => {
                crate::geoip::update(&self.cancel)?;
                live.reload_geoip()
            }
        }
    }
    pub fn terminate(
        &mut self,
        request: &crate::process::TerminationRequest,
    ) -> Result<crate::process::TerminationReport> {
        ensure!(
            !self.cancel.load(Ordering::Relaxed),
            "termination cancelled"
        );
        match &mut self.state {
            State::Demo(demo) => demo.terminate(request),
            State::Live(_) => crate::process::terminate(request),
        }
    }
    pub fn mutate(&mut self, mutation: Mutation) -> Result<()> {
        ensure!(!self.cancel.load(Ordering::Relaxed), "mutation cancelled");
        if let State::Demo(demo) = &mut self.state {
            return demo.mutate(mutation);
        }
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
