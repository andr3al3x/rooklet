//! Application path validation and labels for incoming permissions.
use anyhow::{Context, Result, ensure};
use std::{collections::HashSet, path::Path, sync::atomic::AtomicBool};

const MAX_PLIST_BYTES: u64 = 1024 * 1024;

pub fn validate_existing_path(value: &str) -> Result<()> {
    rooklet_core::application::validate_path(value)?;
    let path = Path::new(value);
    let metadata = std::fs::metadata(path).context("application path does not exist")?;
    if metadata.is_dir() {
        ensure!(
            path.extension().is_some_and(|extension| extension == "app"),
            "directory must be an application bundle (.app)"
        );
    } else {
        ensure!(
            metadata.is_file(),
            "application path must be a bundle or executable file"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            ensure!(
                metadata.permissions().mode() & 0o111 != 0,
                "application file is not executable"
            );
        }
    }

    Ok(())
}

pub(crate) fn validate_application_targets(paths: &[String]) -> Result<()> {
    ensure!(
        !paths.is_empty() && paths.len() <= 256,
        "application action requires between 1 and 256 registered paths"
    );
    let mut unique = HashSet::new();
    for path in paths {
        validate_existing_path(path)
            .with_context(|| format!("invalid application target {path:?}"))?;
        ensure!(unique.insert(path), "duplicate application target {path:?}");
    }
    Ok(())
}

/// Resolve the precise executable that ALF should register, using bundle metadata.
/// This performs bounded read-only filesystem and `plutil` queries.
pub fn registration_path(value: &str) -> Result<String> {
    registration_path_with(value, &mut |plist| {
        crate::command::run(
            Path::new("/usr/bin/plutil"),
            &[
                "-extract".into(),
                "CFBundleExecutable".into(),
                "raw".into(),
                "-expect".into(),
                "string".into(),
                "-o".into(),
                "-".into(),
                plist.to_owned(),
            ],
            None,
            false,
            &AtomicBool::new(false),
        )
    })
}

pub(crate) fn registration_path_with(
    value: &str,
    read_executable: &mut impl FnMut(&str) -> Result<String>,
) -> Result<String> {
    validate_existing_path(value)?;
    let path = Path::new(value);
    let executable = if path.is_dir() {
        let plist = path.join("Contents/Info.plist");
        let metadata = std::fs::metadata(&plist).context("application bundle lacks Info.plist")?;
        ensure!(
            metadata.is_file() && metadata.len() > 0 && metadata.len() <= MAX_PLIST_BYTES,
            "application Info.plist must be a nonempty file no larger than 1 MiB"
        );
        let plist = plist.to_str().context("Info.plist path is not UTF-8")?;
        let output = read_executable(plist).context("unable to read bundle CFBundleExecutable")?;
        // plutil emits one terminating newline; preserve whitespace within a valid name.
        let name = output.strip_suffix('\n').unwrap_or(&output);
        ensure!(
            !name.is_empty()
                && name.len() < 4096
                && name != "."
                && name != ".."
                && !name.contains('/')
                && !name.chars().any(char::is_control),
            "bundle CFBundleExecutable must be a single valid filename"
        );
        let executable = path.join("Contents/MacOS").join(name);
        validate_existing_path(
            executable
                .to_str()
                .context("bundle executable path is not UTF-8")?,
        )?;
        ensure!(executable.is_file(), "bundle executable must be a file");
        executable
    } else {
        path.to_path_buf()
    };
    let canonical =
        std::fs::canonicalize(executable).context("unable to resolve executable path")?;
    let canonical = canonical
        .to_str()
        .context("resolved executable path is not UTF-8")?;
    validate_existing_path(canonical)?;
    Ok(canonical.to_owned())
}

#[cfg(test)]
#[path = "application/tests.rs"]
mod tests;
pub(crate) fn application_name(path: &str) -> String {
    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path);
    name.strip_suffix(".app").unwrap_or(name).to_owned()
}
