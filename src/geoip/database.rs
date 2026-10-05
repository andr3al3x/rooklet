//! Managed filesystem access and provider/schema validation.
use anyhow::{Context, Result, ensure};
use maxminddb::{Reader, WithinOptions, geoip2};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub(super) const MAX_DATABASE: usize = 64 * 1024 * 1024;
const MAX_NETWORKS: usize = 4_000_000;
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FileStamp {
    length: u64,
    modified: SystemTime,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}
pub(super) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub(super) fn description(build_epoch: u64) -> String {
    format!(
        "DB-IP Lite · {} days old · CC BY 4.0",
        unix_now().saturating_sub(build_epoch) / 86_400
    )
}
pub(super) fn managed_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .context("unable to resolve the home directory for managed GeoIP data")?;
    let home = PathBuf::from(home);
    ensure!(home.is_absolute(), "home directory must be absolute");
    let home = home
        .canonicalize()
        .context("home directory is unavailable")?;
    Ok(home.join("Library/Application Support/rooklet/geoip/Country.mmdb"))
}
fn check_parents(path: &Path, create: bool) -> Result<()> {
    // Only create managed descendants; the user's home must already exist.
    let directory = path.parent().context("invalid managed database path")?;
    let home = directory
        .ancestors()
        .nth(4)
        .context("invalid managed database location")?;
    ensure!(home.is_dir(), "home directory is unavailable");
    let mut current = home.to_path_buf();
    for name in ["Library", "Application Support", "rooklet", "geoip"] {
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "managed GeoIP directory is not a real directory"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
                let mut builder = fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                match builder.create(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        let metadata = fs::symlink_metadata(&current)?;
                        ensure!(
                            metadata.is_dir() && !metadata.file_type().is_symlink(),
                            "managed GeoIP directory is not a real directory"
                        );
                    }
                    Err(error) => {
                        return Err(error).context("cannot create managed GeoIP directory");
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).context("cannot inspect managed GeoIP directory"),
        }
    }
    Ok(())
}
pub(super) fn file_stamp(path: &Path) -> Result<Option<FileStamp>> {
    check_parents(path, false)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("cannot inspect managed GeoIP database"),
    };
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "managed GeoIP database must be a regular file, never a symlink"
    );
    ensure!(
        metadata.len() <= MAX_DATABASE as u64,
        "GeoIP database exceeds 64 MiB"
    );
    Ok(Some(FileStamp {
        length: metadata.len(),
        modified: metadata.modified()?,
        #[cfg(unix)]
        device: metadata.dev(),
        #[cfg(unix)]
        inode: metadata.ino(),
    }))
}
pub(super) fn load(path: &Path) -> Result<Reader<Vec<u8>>> {
    file_stamp(path)?.context("managed GeoIP database is unavailable")?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options
        .open(path)
        .context("cannot open managed GeoIP database")?;
    ensure!(
        file.metadata()?.is_file(),
        "managed GeoIP database must be a regular file"
    );
    let mut bytes = Vec::new();
    file.take(MAX_DATABASE as u64 + 1)
        .read_to_end(&mut bytes)
        .context("cannot read managed GeoIP database")?;
    validate(
        bytes,
        &AtomicBool::new(false),
        Instant::now() + Duration::from_secs(30),
    )
}
pub(super) fn validate(
    bytes: Vec<u8>,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Reader<Vec<u8>>> {
    validate_source(&bytes, cancel, deadline)?;
    Reader::from_source(bytes).context("invalid DB-IP database")
}
pub(super) fn validate_source<'a>(
    bytes: &'a [u8],
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Reader<&'a [u8]>> {
    super::download::check(cancel, deadline)?;
    ensure!(bytes.len() <= MAX_DATABASE, "GeoIP database exceeds 64 MiB");
    let reader = Reader::from_source(bytes).context("invalid GeoIP MMDB")?;
    let metadata = &reader.metadata;
    ensure!(
        metadata.database_type == "DBIP-Country-Lite"
            && metadata
                .description
                .values()
                .any(|description| description.contains("DB-IP.com")),
        "managed GeoIP requires the DB-IP Country Lite database"
    );
    ensure!(
        metadata.binary_format_major_version == 2
            && metadata.binary_format_minor_version == 0
            && metadata.ip_version == 6
            && metadata.node_count != 0,
        "unsupported DB-IP database metadata"
    );
    ensure!(
        metadata.build_epoch != 0 && metadata.build_epoch <= unix_now() + 86_400,
        "invalid DB-IP database build time"
    );
    let tree_end = (metadata.node_count as usize)
        .checked_mul(metadata.record_size as usize)
        .context("invalid DB-IP tree size")?
        / 4;
    let separator = bytes
        .get(tree_end..tree_end.saturating_add(16))
        .context("truncated DB-IP database tree")?;
    ensure!(
        separator.iter().all(|byte| *byte == 0),
        "invalid DB-IP data separator"
    );
    let mut country_count = 0;
    for (index, record) in reader
        .networks(WithinOptions::default().include_networks_without_data())?
        .enumerate()
    {
        ensure!(index < MAX_NETWORKS, "DB-IP database has too many networks");
        super::download::check(cancel, deadline)?;
        let record = record.context("invalid DB-IP database search tree")?;
        if !record.has_data() {
            continue;
        }
        let country = record
            .decode::<geoip2::Country>()
            .context("invalid DB-IP country record")?
            .context("missing DB-IP country record")?;
        // Empty geographic fields remain Unknown rather than inventing a country.
        if let Some(code) = country.country.iso_code {
            ensure!(
                code.len() == 2 && code.bytes().all(|byte| byte.is_ascii_uppercase()),
                "invalid DB-IP country code"
            );
            if let Some(name) = country.country.names.english {
                ensure!(
                    !name.is_empty() && name.len() <= 1024,
                    "invalid DB-IP country name"
                );
            }
            country_count += 1;
        }
    }
    ensure!(
        country_count != 0,
        "DB-IP database contains no country records"
    );
    super::download::check(cancel, deadline)?;
    Ok(reader)
}
pub(super) fn install(
    path: &Path,
    bytes: &[u8],
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<()> {
    super::download::check(cancel, deadline)?;
    check_parents(path, true)?;
    file_stamp(path)?;
    let directory = path
        .parent()
        .context("invalid managed database directory")?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".Country-")
        .suffix(".mmdb")
        .tempfile_in(directory)
        .context("cannot create temporary GeoIP database")?;
    temporary
        .write_all(bytes)
        .context("cannot write temporary GeoIP database")?;
    temporary
        .as_file()
        .sync_all()
        .context("cannot sync temporary GeoIP database")?;
    // Recheck the destination immediately before the only externally visible change.
    file_stamp(path)?;
    super::download::check(cancel, deadline)?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .context("cannot atomically install GeoIP database")?;
    // Installation has committed; cancellation cannot turn success into a failed update.
    if let Ok(directory) = File::open(directory) {
        let _ = directory.sync_all();
    }
    Ok(())
}
