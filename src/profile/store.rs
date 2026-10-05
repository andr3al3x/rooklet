//! Bounded private named profiles using directory-relative, no-follow operations.
use super::preparation::{MAX_BYTES, read_file, validate};
use crate::model::Profile;
use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::{CStr, CString, OsStr},
    fs::{File, OpenOptions},
    io::Write,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawFd, FromRawFd},
    },
    path::{Component, Path, PathBuf},
};
const MAX_PROFILES: usize = 256;
const MAX_ENTRIES: usize = 1024;

pub(super) fn list() -> Result<Vec<String>> {
    list_at(&directory()?)
}
pub(super) fn load(name: &str) -> Result<Profile> {
    load_at(&directory()?, name)
}
pub(super) fn save(name: &str, profile: &Profile) -> Result<()> {
    save_at(&directory()?, name, profile)
}
fn directory() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is unavailable")?;
    let home = PathBuf::from(home);
    ensure!(home.is_absolute(), "HOME must be an absolute path");
    Ok(home.join("Library/Application Support/rooklet/profiles"))
}
fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.trim().is_empty()
            && name.len() <= 64
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b" _-".contains(&byte)),
        "profile name must contain 1–64 ASCII letters, digits, spaces, underscores or hyphens"
    );
    Ok(())
}
fn open_directory(path: &Path, create: bool) -> Result<Option<File>> {
    ensure!(path.is_absolute(), "profile directory must be absolute");
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut directory = options.open("/")?;
    for component in path.components() {
        let Component::Normal(name) = component else {
            ensure!(
                component == Component::RootDir,
                "profile directory must be normalized"
            );
            continue;
        };
        let name = CString::new(name.as_bytes())?;
        let mut fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::NotFound {
                if !create {
                    return Ok(None);
                }
                let result = unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) };
                if result < 0
                    && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
                {
                    return Err(std::io::Error::last_os_error().into());
                }
                fd = unsafe {
                    libc::openat(
                        directory.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
            }
        }
        ensure!(
            fd >= 0,
            "cannot open profile directory safely: {}",
            std::io::Error::last_os_error()
        );
        directory = unsafe { File::from_raw_fd(fd) };
    }
    let metadata = directory.metadata()?;
    ensure!(
        metadata.uid() == unsafe { libc::geteuid() },
        "profile directory must be owned by the current user"
    );
    ensure!(
        metadata.mode() & 0o077 == 0,
        "profile directory must be private (mode 0700)"
    );
    Ok(Some(directory))
}
fn open_profile(directory: &File, name: &OsStr) -> Result<File> {
    let name = CString::new(name.as_bytes())?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    ensure!(
        fd >= 0,
        "cannot open saved profile safely: {}",
        std::io::Error::last_os_error()
    );
    let file = unsafe { File::from_raw_fd(fd) };
    ensure!(
        file.metadata()?.is_file(),
        "saved profile must be a regular file"
    );
    Ok(file)
}
fn names(directory: &File) -> Result<Vec<String>> {
    // fdopendir consumes its fd; retain our trusted descriptor for later openat calls.
    let fd = unsafe { libc::dup(directory.as_raw_fd()) };
    ensure!(fd >= 0, "cannot enumerate profile directory");
    let pointer = unsafe { libc::fdopendir(fd) };
    if pointer.is_null() {
        unsafe {
            libc::close(fd);
        }
        bail!("cannot enumerate profile directory");
    }
    struct Directory(*mut libc::DIR);
    impl Drop for Directory {
        fn drop(&mut self) {
            unsafe {
                libc::closedir(self.0);
            }
        }
    }
    let directory_stream = Directory(pointer);
    let mut names = Vec::new();
    let mut count = 0;
    loop {
        let entry = unsafe { libc::readdir(directory_stream.0) };
        if entry.is_null() {
            break;
        }
        let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        count += 1;
        ensure!(
            count <= MAX_ENTRIES,
            "profile directory contains too many entries"
        );
        let Ok(filename) = std::str::from_utf8(bytes) else {
            continue;
        };
        let Some(name) = filename.strip_suffix(".json") else {
            continue;
        };
        validate_name(name)?;
        let file = open_profile(directory, OsStr::new(filename))?;
        ensure!(
            file.metadata()?.len() <= MAX_BYTES as u64,
            "saved profile exceeds 1 MiB"
        );
        names.push(name.to_owned());
        ensure!(
            names.len() <= MAX_PROFILES,
            "at most {MAX_PROFILES} saved profiles are supported"
        );
    }
    names.sort();
    Ok(names)
}
fn list_at(path: &Path) -> Result<Vec<String>> {
    let Some(directory) = open_directory(path, false)? else {
        return Ok(Vec::new());
    };
    names(&directory)
}
fn load_at(path: &Path, name: &str) -> Result<Profile> {
    validate_name(name)?;
    let directory =
        open_directory(path, false)?.context("saved profile directory does not exist")?;
    // Enforce the same count bound for direct loads as for listing.
    names(&directory)?;
    read_file(open_profile(
        &directory,
        OsStr::new(&format!("{name}.json")),
    )?)
}
fn save_lock(directory: &File) -> Result<File> {
    let name = CString::new(".save.lock")?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            0o600,
        )
    };
    ensure!(
        fd >= 0,
        "cannot open profile save lock safely: {}",
        std::io::Error::last_os_error()
    );
    let lock = unsafe { File::from_raw_fd(fd) };
    let metadata = lock.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o077 == 0,
        "profile save lock must be a private regular file owned by the current user"
    );
    let result = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    ensure!(
        result == 0,
        "another profile save is in progress: {}",
        std::io::Error::last_os_error()
    );
    Ok(lock)
}
fn save_at(path: &Path, name: &str, profile: &Profile) -> Result<()> {
    validate_name(name)?;
    validate(profile)?;
    let bytes = serde_json::to_vec_pretty(profile)?;
    ensure!(bytes.len() < MAX_BYTES, "configuration exceeds 1 MiB");
    let directory =
        open_directory(path, true)?.context("saved profile directory is unavailable")?;
    let _lock = save_lock(&directory)?;
    ensure!(
        names(&directory)?.len() < MAX_PROFILES,
        "at most {MAX_PROFILES} saved profiles are supported"
    );
    let destination = CString::new(format!("{name}.json"))?;
    let mut temporary = None;
    for sequence in 0..128 {
        let candidate = CString::new(format!(".profile-{}-{sequence}.tmp", std::process::id()))?;
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                candidate.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd >= 0 {
            temporary = Some((candidate, unsafe { File::from_raw_fd(fd) }));
            break;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(error.into());
        }
    }
    let (temporary_name, mut file) =
        temporary.context("cannot create private profile temporary file")?;
    let result = (|| -> Result<()> {
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        // linkat publishes the complete file atomically and refuses any existing name.
        let linked = unsafe {
            libc::linkat(
                directory.as_raw_fd(),
                temporary_name.as_ptr(),
                directory.as_raw_fd(),
                destination.as_ptr(),
                0,
            )
        };
        ensure!(
            linked == 0,
            "cannot save profile; names are never overwritten: {}",
            std::io::Error::last_os_error()
        );
        directory.sync_all()?;
        Ok(())
    })();
    let removed = unsafe { libc::unlinkat(directory.as_raw_fd(), temporary_name.as_ptr(), 0) };
    result?;
    ensure!(
        removed == 0,
        "profile saved, but temporary-file cleanup failed: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    fn profile() -> Profile {
        Profile {
            format: "rooklet-profile".into(),
            version: 1,
            firewall: Some(Default::default()),
            applications: vec![],
            network_rules: vec![],
        }
    }
    #[test]
    fn bounded_private_round_trip_never_overwrites() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().canonicalize().unwrap().join("profiles");
        assert!(list_at(&directory).unwrap().is_empty());
        save_at(&directory, "Work profile", &profile()).unwrap();
        assert_eq!(list_at(&directory).unwrap(), vec!["Work profile"]);
        assert_eq!(
            load_at(&directory, "Work profile").unwrap().format,
            "rooklet-profile"
        );
        assert!(save_at(&directory, "Work profile", &profile()).is_err());
        assert_eq!(
            std::fs::metadata(directory.join("Work profile.json"))
                .unwrap()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(std::fs::metadata(&directory).unwrap().mode() & 0o777, 0o700);
    }
    #[test]
    fn rejects_traversal_symlinks_nonfiles_and_large_files() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().canonicalize().unwrap().join("profiles");
        for name in ["../escape", "", ".", "a/b", "a\n", "é", &"a".repeat(65)] {
            assert!(save_at(&directory, name, &profile()).is_err());
        }
        save_at(&directory, "valid", &profile()).unwrap();
        symlink(directory.join("valid.json"), directory.join("alias.json")).unwrap();
        assert!(load_at(&directory, "alias").is_err());
        std::fs::remove_file(directory.join("alias.json")).unwrap();
        std::fs::create_dir(directory.join("folder.json")).unwrap();
        assert!(load_at(&directory, "folder").is_err());
        std::fs::remove_dir(directory.join("folder.json")).unwrap();
        std::fs::write(directory.join("large.json"), vec![b' '; MAX_BYTES + 1]).unwrap();
        assert!(load_at(&directory, "large").is_err());
        let alias = temporary.path().canonicalize().unwrap().join("symlink");
        symlink(&directory, &alias).unwrap();
        assert!(save_at(&alias, "other", &profile()).is_err());
    }
    #[test]
    fn rejects_public_directory_and_excess_entries() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().canonicalize().unwrap().join("profiles");
        save_at(&directory, "valid", &profile()).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(list_at(&directory).is_err());
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        for index in 0..MAX_PROFILES {
            std::fs::write(directory.join(format!("{index}.json")), b"{}").unwrap();
        }
        assert!(list_at(&directory).is_err());
        assert!(load_at(&directory, "valid").is_err());
    }
}
