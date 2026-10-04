//! Explicit database updates do not initialize firewall adapters or require sudo.
use super::args::GeoipCommand;
use anyhow::Result;
use std::sync::atomic::AtomicBool;

pub(super) fn run(command: GeoipCommand, demo: bool) -> Result<()> {
    match command {
        GeoipCommand::Update if demo => {
            println!("Demo mode; country database update simulated. No download or files changed.");
        }
        GeoipCommand::Update => {
            println!("Updating country database…");
            println!("{}", xield::geoip::update(&AtomicBool::new(false))?);
        }
    }
    Ok(())
}
