//! Pure syntax checks for macOS application registration paths.
use anyhow::{Result, ensure};

/// Require an absolute, normalized slash-delimited path without consulting a filesystem.
pub fn validate_path(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty() && value.len() < 4096 && !value.chars().any(char::is_control),
        "invalid application path"
    );
    ensure!(
        value.starts_with('/')
            && value != "/"
            && !value[1..]
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == ".."),
        "application path must be an absolute normalized path"
    );
    Ok(())
}
