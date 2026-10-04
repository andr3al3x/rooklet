//! Bounded, root-owned persistent state and atomic file replacement.
use super::{CONFIG, MAX_FILE, compiler::validate_rules, pf::valid_token};
use crate::model::NetworkRule;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const STATE_DIR: &str = "/Library/Application Support/Xield";
pub(super) const STATE_FILE: &str = "/Library/Application Support/Xield/network.json";

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct State {
    pub(super) rules: Vec<NetworkRule>,
    pub(super) enable_token: Option<String>,
    pub(super) active: bool,
    pub(super) loaded_rules: Option<String>,
}

pub(super) fn bounded_read(path: &Path) -> Result<String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .with_context(|| format!("Cannot read {}", path.display()))?;
    ensure!(
        file.metadata()?.is_file(),
        "{} is not a regular file",
        path.display()
    );
    let mut bytes = Vec::new();
    (&mut file).take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_FILE,
        "{} exceeds the 1 MiB safety limit",
        path.display()
    );
    String::from_utf8(bytes).context("PF configuration contains invalid UTF-8")
}
pub(super) fn trusted(path: &Path, directory: bool) -> Result<()> {
    let meta =
        fs::symlink_metadata(path).with_context(|| format!("Cannot inspect {}", path.display()))?;
    ensure!(
        !meta.file_type().is_symlink(),
        "Refusing symlink {}",
        path.display()
    );
    ensure!(
        if directory {
            meta.is_dir()
        } else {
            meta.is_file()
        },
        "Unexpected file type at {}",
        path.display()
    );
    ensure!(
        meta.uid() == 0 && meta.mode() & 0o022 == 0,
        "{} must be root-owned and not writable by other users",
        path.display()
    );
    Ok(())
}
pub(super) fn trusted_parents(path: &Path) -> Result<()> {
    // macOS's /etc is an intentional system symlink to /private/etc.
    let parent = path.parent().context("Missing parent directory")?;
    let canonical = fs::canonicalize(parent)?;
    if parent.starts_with("/etc") {
        ensure!(
            canonical.starts_with("/private/etc") || canonical.starts_with("/etc"),
            "Unexpected /etc destination"
        );
    } else {
        ensure!(
            canonical == parent,
            "Refusing a symlink in {}",
            parent.display()
        );
    }
    for component in canonical.ancestors() {
        trusted(component, true)?;
    }
    Ok(())
}
pub(super) fn atomic_write(path: &Path, data: &[u8], mode: u32) -> Result<()> {
    ensure!(
        data.len() as u64 <= MAX_FILE,
        "{} exceeds the 1 MiB safety limit",
        path.display()
    );
    trusted_parents(path)?;
    if path.exists() || fs::symlink_metadata(path).is_ok() {
        trusted(path, false)?;
    }
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temporary = path.with_file_name(format!(".xield-{}-{nonce}.tmp", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)?;
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::File::open(path.parent().unwrap())?.sync_all()?;
        Ok(())
    })();
    let _ = fs::remove_file(temporary);
    result
}
pub(super) fn prepare_state() -> Result<()> {
    let path = Path::new(STATE_DIR);
    trusted_parents(path)?;
    match fs::create_dir(path) {
        Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(0o700))?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    trusted(path, true)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub(super) fn mutation_lock() -> Result<fs::File> {
    prepare_state()?;
    let path = Path::new(STATE_DIR).join(".network.lock");
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)?;
    trusted(&path, false)?;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "Another Xield network change is in progress; retry after it completes"
    );
    Ok(lock)
}
pub(super) fn read_state() -> Result<State> {
    let path = Path::new(STATE_FILE);
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(error) => Err(error.into()),
        Ok(_) => {
            trusted_parents(path)?;
            trusted(path, false)?;
            let state: State = serde_json::from_str(&bounded_read(path)?)
                .context("Invalid Xield network state")?;
            validate_rules(&state.rules)?;
            if let Some(token) = &state.enable_token {
                ensure!(valid_token(token), "Invalid stored PF enable token");
            }
            Ok(state)
        }
    }
}
pub(super) fn save_state(state: &State) -> Result<()> {
    atomic_write(
        Path::new(STATE_FILE),
        &serde_json::to_vec_pretty(state)?,
        0o600,
    )
}

pub(super) fn backup_config(source: &str) -> Result<PathBuf> {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = PathBuf::from(format!("{CONFIG}.xield-backup-{timestamp}"));
    atomic_write(&path, source.as_bytes(), 0o600)?;
    Ok(path)
}
