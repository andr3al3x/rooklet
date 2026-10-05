//! Administrator authentication through the user's terminal.
use anyhow::{Context, Result, ensure};
use std::process::Command;

pub(crate) fn authenticate() -> Result<()> {
    if rooklet_macos::is_root() {
        tracing::debug!("authentication skipped for privileged session");
        return Ok(());
    }
    tracing::info!("administrator authentication requested");
    let started = std::time::Instant::now();
    let status = Command::new("/usr/bin/sudo")
        .arg("-v")
        .status()
        .inspect_err(|_| tracing::error!("authentication subprocess could not start"))
        .context("cannot start sudo authentication")?;
    tracing::info!(
        accepted = status.success(),
        duration_ms = started.elapsed().as_millis() as u64,
        "administrator authentication completed"
    );
    ensure!(
        status.success(),
        "administrator authentication was cancelled or denied"
    );
    Ok(())
}
