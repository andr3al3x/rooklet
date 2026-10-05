//! Administrator authentication through the user's terminal.
use anyhow::{Context, Result, ensure};
use std::process::Command;

pub(crate) fn authenticate() -> Result<()> {
    if rooklet_macos::is_root() {
        return Ok(());
    }
    let status = Command::new("/usr/bin/sudo")
        .arg("-v")
        .status()
        .context("cannot start sudo authentication")?;
    ensure!(
        status.success(),
        "administrator authentication was cancelled or denied"
    );
    Ok(())
}
