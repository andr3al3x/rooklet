use std::ffi::{CStr, CString};
use std::fs::{DirBuilder, File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

const MAX_DIRECTORY_ENTRIES: usize = 4096;
static RUN_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn create_run(path: &Path, retained_files: usize) -> Result<File> {
    // Strip trailing separators/dot components so O_NOFOLLOW still applies to
    // the selected directory itself ("linked-dir/" must not bypass it).
    let path: std::path::PathBuf = path.components().collect();
    match DirBuilder::new().mode(0o700).create(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error).context("could not create diagnostic log directory"),
    }
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)
        .context("could not open diagnostic log directory (symlinks are refused)")?;
    let metadata = directory
        .metadata()
        .context("could not inspect diagnostic log directory")?;
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    if metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        bail!("diagnostic log directory must be owned by the current user and private (mode 0700)");
    }
    // Only startup is serialized. Active CLI and TUI sessions may share a directory.
    let _lock = lock_directory(&directory, uid)?;
    retain(&directory, retained_files.saturating_sub(1), uid)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("could not determine diagnostic log run time")?
        .as_nanos();
    let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = CString::new(format!(
        "rooklet-{now:020}-{:010}-{sequence:010}.jsonl",
        std::process::id()
    ))?;
    // SAFETY: directory owns a live directory fd, name is NUL terminated, and
    // create_new/O_NOFOLLOW prevents opening a pre-existing or linked file.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error()).context("could not create diagnostic log file");
    }
    // SAFETY: openat returned a new owned fd.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn lock_directory(directory: &File, uid: u32) -> Result<File> {
    // SAFETY: directory is a live fd; the static lock name is NUL terminated.
    // NONBLOCK avoids waiting if an untrusted entry is a special file.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            c".rooklet.lock".as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error())
            .context("could not open diagnostic log directory lock");
    }
    // SAFETY: openat returned a new owned fd.
    let lock = unsafe { File::from_raw_fd(fd) };
    let metadata = lock
        .metadata()
        .context("could not inspect diagnostic log directory lock")?;
    if !metadata.is_file()
        || metadata.uid() != uid
        || metadata.mode() & 0o077 != 0
        || metadata.nlink() != 1
    {
        bail!("diagnostic log directory lock must be a private, regular, current-user file");
    }
    // SAFETY: lock owns the live regular-file fd.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::last_os_error())
            .context("diagnostic log directory is already in use");
    }
    Ok(lock)
}

struct DirectoryEntries(*mut libc::DIR);

impl Drop for DirectoryEntries {
    fn drop(&mut self) {
        // SAFETY: fdopendir returned this owned directory stream.
        unsafe { libc::closedir(self.0) };
    }
}

fn retain(directory: &File, keep: usize, uid: u32) -> Result<()> {
    // SAFETY: directory owns a live fd. fdopendir takes ownership of the duplicate.
    let fd = unsafe { libc::fcntl(directory.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error())
            .context("could not inspect diagnostic log retention");
    }
    // SAFETY: fd is an owned duplicate of a directory fd.
    let stream = unsafe { libc::fdopendir(fd) };
    if stream.is_null() {
        let error = io::Error::last_os_error();
        // SAFETY: fdopendir failed and did not take ownership.
        unsafe { libc::close(fd) };
        return Err(error).context("could not inspect diagnostic log retention");
    }
    let entries = DirectoryEntries(stream);
    let mut names = Vec::new();
    let mut count = 0;
    loop {
        // SAFETY: this thread's errno pointer is always live and writable.
        unsafe { *errno_pointer() = 0 };
        // SAFETY: entries owns a live stream; the returned entry is read only
        // until the next readdir call, and d_name is NUL terminated.
        let entry = unsafe { libc::readdir(entries.0) };
        if entry.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(0) {
                return Err(error).context("could not read diagnostic log directory");
            }
            break;
        }
        count += 1;
        if count > MAX_DIRECTORY_ENTRIES {
            bail!("diagnostic log directory contains too many entries");
        }
        // SAFETY: readdir returned a live entry with a NUL-terminated name.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if !is_run_name(name.to_bytes()) {
            continue;
        }
        // Inspect entries relative to the verified directory without following
        // symlinks. Retention must never remove an unrelated entry.
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: metadata is writable and name points to a live NUL-terminated
        // directory entry; fstatat initializes metadata on success.
        let result = unsafe {
            libc::fstatat(
                directory.as_raw_fd(),
                name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error())
                .context("could not inspect retained diagnostic log");
        }
        // SAFETY: successful fstatat initialized metadata.
        let metadata = unsafe { metadata.assume_init() };
        if metadata.st_mode & libc::S_IFMT != libc::S_IFREG
            || metadata.st_uid != uid
            || metadata.st_mode & 0o077 != 0
            || metadata.st_nlink != 1
        {
            bail!("retained diagnostic log is not a private, regular, current-user file");
        }
        names.push(name.to_owned());
    }
    names.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    for name in names.iter().take(names.len().saturating_sub(keep)) {
        // SAFETY: removal is contained in the verified directory fd and only
        // names matching the exact owned run-file pattern were collected.
        if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error())
                .context("could not remove expired diagnostic log");
        }
    }
    Ok(())
}

fn errno_pointer() -> *mut libc::c_int {
    #[cfg(target_os = "macos")]
    // SAFETY: __error returns the current thread's errno pointer.
    unsafe {
        libc::__error()
    }
    #[cfg(not(target_os = "macos"))]
    // SAFETY: __errno_location returns the current thread's errno pointer.
    unsafe {
        libc::__errno_location()
    }
}

fn is_run_name(name: &[u8]) -> bool {
    let Some(inner) = name
        .strip_prefix(b"rooklet-")
        .and_then(|name| name.strip_suffix(b".jsonl"))
    else {
        return false;
    };
    let mut components = inner.split(|byte| *byte == b'-');
    [20, 10, 10].into_iter().all(|length| {
        components
            .next()
            .is_some_and(|part| part.len() == length && part.iter().all(u8::is_ascii_digit))
    }) && components.next().is_none()
}

#[cfg(test)]
pub(super) fn run_files(path: &Path) -> Vec<std::path::PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut files: Vec<_> = std::fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| is_run_name(name.as_bytes()))
        })
        .collect();
    files.sort();
    files
}
