//! Explicit database updates do not initialize firewall adapters or require sudo.
use super::args::GeoipCommand;
use anyhow::Result;
use std::sync::atomic::AtomicBool;

pub(super) fn run(command: GeoipCommand) -> Result<()> {
    match command {
        GeoipCommand::Update => {
            println!("Updating country database…");
            println!(
                "{}",
                rooklet_core::text::clean(&rooklet_macos::geoip::update(&AtomicBool::new(false))?)
            );
        }
    }
    Ok(())
}
