use super::download::{Download, Month};
use super::*;
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(30)
}
fn replace(bytes: &mut Vec<u8>, original: &[u8], replacement: &[u8]) {
    let start = bytes
        .windows(original.len())
        .position(|window| window == original)
        .unwrap();
    bytes.splice(start..start + original.len(), replacement.iter().copied());
}
fn provider_fixture() -> Vec<u8> {
    // Synthetic DB-IP metadata over the existing licensed MaxMind test records;
    // this is schema/installation test data, never shipped provider geography.
    let mut bytes = include_bytes!("../../tests/data/GeoIP2-Country-Test.mmdb").to_vec();
    replace(&mut bytes, b"\x4eGeoIP2-Country", b"\x51DBIP-Country-Lite");
    replace(
        &mut bytes,
        b"\x5d\x2dGeoIP2 Country Test Database (fake GeoIP2 data, for example purposes only)",
        b"\x59DB-IP.com - IP to Country",
    );
    // The original metadata aliases its English language string by offset;
    // changing earlier string lengths requires materializing this one value.
    replace(
        &mut bytes,
        b"\x49languages\x01\x04\x20\x78",
        b"\x49languages\x01\x04\x42en",
    );
    bytes
}
fn archive(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}
fn managed_path(home: &Path) -> PathBuf {
    home.join("Library/Application Support/rooklet/geoip/Country.mmdb")
}
fn install_fixture(path: &Path) {
    let bytes = provider_fixture();
    database::validate_source(&bytes, &AtomicBool::new(false), deadline()).unwrap();
    database::install(path, &bytes, &AtomicBool::new(false), deadline()).unwrap();
}
fn managed(path: PathBuf) -> GeoIp {
    GeoIp {
        path: Some(path),
        ..GeoIp::default()
    }
}

#[test]
fn provider_schema_supports_ipv4_ipv6_and_bounded_cache() {
    let mut database = GeoIp {
        reader: Some(
            database::validate(provider_fixture(), &AtomicBool::new(false), deadline()).unwrap(),
        ),
        ..GeoIp::default()
    };
    let sweden = database.lookup("89.160.20.112".parse().unwrap()).unwrap();
    assert_eq!(sweden.code, "SE");
    assert_eq!(sweden.name, "Sweden");
    let japan = database.lookup("2001:218::".parse().unwrap()).unwrap();
    assert_eq!(japan.code, "JP");
    assert_eq!(database.lookup("2001:218::".parse().unwrap()), Some(japan));
    assert!(database.lookup("192.168.0.1".parse().unwrap()).is_none());
    assert!(database.lookup("1.1.1.1".parse().unwrap()).is_none());
    for index in 0..5000 {
        database.lookup(IpAddr::V4(std::net::Ipv4Addr::from(0x08000000 + index)));
    }
    assert_eq!(database.cache.len(), CACHE_LIMIT);
    assert_eq!(database.order.len(), CACHE_LIMIT);
    assert!(database.description().unwrap().contains("DB-IP Lite"));
    assert!(database.description().unwrap().contains("CC BY 4.0"));
}
#[test]
fn managed_missing_reload_and_invalid_replacement_preserve_good_state() {
    let home = tempfile::tempdir().unwrap();
    let path = managed_path(home.path());
    let mut database = managed(path.clone());
    database.refresh().unwrap();
    assert!(database.description().is_none());
    install_fixture(&path);
    database.refresh().unwrap();
    assert!(database.lookup("89.160.20.112".parse().unwrap()).is_some());
    let old_stamp = database.stamp.clone();
    let cache = database.cache.clone();
    database.refresh().unwrap();
    assert_eq!(database.stamp, old_stamp);
    assert_eq!(database.cache, cache);
    install_fixture(&path);
    database.refresh().unwrap();
    assert!(database.cache.is_empty());
    assert_ne!(database.stamp, old_stamp);
    let current_stamp = database.stamp.clone();
    let current = database.lookup("89.160.20.112".parse().unwrap());
    fs::write(&path, b"not an MMDB").unwrap();
    assert!(database.refresh().is_err());
    assert_eq!(database.stamp, current_stamp);
    assert_eq!(database.lookup("89.160.20.112".parse().unwrap()), current);
    fs::remove_file(path).unwrap();
    database.refresh().unwrap();
    assert!(database.cache.is_empty());
    assert!(database.description().is_none());
}
#[test]
fn invalid_provider_and_country_schema_are_rejected() {
    let original = include_bytes!("../../tests/data/GeoIP2-Country-Test.mmdb").to_vec();
    assert!(database::validate(original, &AtomicBool::new(false), deadline()).is_err());
    let mut bytes = provider_fixture();
    replace(&mut bytes, b"\x42SE", b"\x42sE");
    assert!(database::validate(bytes, &AtomicBool::new(false), deadline()).is_err());
    assert!(database::validate(vec![0; 100], &AtomicBool::new(false), deadline()).is_err());
}
#[test]
fn failed_archives_preserve_previous_database_and_cleanup_temporary_files() {
    let home = tempfile::tempdir().unwrap();
    let path = managed_path(home.path());
    install_fixture(&path);
    let previous = fs::read(&path).unwrap();
    let mut truncated = archive(&provider_fixture());
    truncated.truncate(truncated.len() - 4);
    let mut trailing = archive(&provider_fixture());
    trailing.push(0);
    let mut oversized = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    for _ in 0..=database::MAX_DATABASE / 8192 {
        oversized.write_all(&[0; 8192]).unwrap();
    }
    for compressed in [
        archive(b"not an MMDB"),
        truncated,
        trailing,
        oversized.finish().unwrap(),
    ] {
        let result = download::update_with(
            &path,
            Month {
                year: 2026,
                month: 10,
            },
            &AtomicBool::new(false),
            |_, _, _| Ok(Download::Archive(compressed.clone())),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), previous);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }
}
#[test]
fn fallback_happens_only_for_month_not_found() {
    let home = tempfile::tempdir().unwrap();
    let path = managed_path(home.path());
    let mut urls = Vec::new();
    let compressed = archive(&provider_fixture());
    let result = download::update_with(
        &path,
        Month {
            year: 2026,
            month: 1,
        },
        &AtomicBool::new(false),
        |url, _, _| {
            urls.push(url.to_string());
            Ok(if urls.len() == 1 {
                Download::NotFound
            } else {
                Download::Archive(compressed.clone())
            })
        },
    )
    .unwrap();
    assert_eq!(
        urls,
        [
            "https://download.db-ip.com/free/dbip-country-lite-2026-01.mmdb.gz",
            "https://download.db-ip.com/free/dbip-country-lite-2025-12.mmdb.gz"
        ]
    );
    assert!(result.contains("https://db-ip.com"));
    let previous = fs::read(&path).unwrap();
    let mut calls = 0;
    assert!(
        download::update_with(
            &path,
            Month {
                year: 2026,
                month: 10
            },
            &AtomicBool::new(false),
            |_, _, _| {
                calls += 1;
                anyhow::bail!("HTTP 500 or transport failure")
            }
        )
        .is_err()
    );
    assert_eq!(calls, 1);
    assert_eq!(fs::read(&path).unwrap(), previous);
}
#[test]
fn cancellation_and_timeout_preserve_database() {
    let home = tempfile::tempdir().unwrap();
    let path = managed_path(home.path());
    install_fixture(&path);
    let previous = fs::read(&path).unwrap();
    let cancel = AtomicBool::new(true);
    let mut called = false;
    assert!(
        download::update_with(
            &path,
            Month {
                year: 2026,
                month: 10
            },
            &cancel,
            |_, _, _| {
                called = true;
                Ok(Download::NotFound)
            }
        )
        .is_err()
    );
    assert!(!called);
    assert!(database::install(&path, b"replacement", &cancel, deadline()).is_err());
    assert_eq!(fs::read(&path).unwrap(), previous);
    cancel.store(false, Ordering::Relaxed);
    struct CancelRead<'a>(&'a AtomicBool);
    impl Read for CancelRead<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            buffer[0] = 1;
            self.0.store(true, Ordering::Relaxed);
            Ok(1)
        }
    }
    assert!(download::read_bounded(CancelRead(&cancel), 100, &cancel, deadline()).is_err());
    cancel.store(false, Ordering::Relaxed);
    assert!(database::validate(provider_fixture(), &cancel, Instant::now()).is_err());
    assert!(download::read_bounded(io::repeat(0), 100, &cancel, deadline()).is_err());
}
#[cfg(unix)]
#[test]
fn managed_database_and_directory_symlinks_are_rejected_without_overwriting_targets() {
    use std::os::unix::fs::symlink;
    let home = tempfile::tempdir().unwrap();
    let path = managed_path(home.path());
    install_fixture(&path);
    let target = home.path().join("unrelated");
    fs::write(&target, "preserve").unwrap();
    fs::remove_file(&path).unwrap();
    symlink(&target, &path).unwrap();
    assert!(managed(path.clone()).refresh().is_err());
    assert!(database::install(&path, b"replacement", &AtomicBool::new(false), deadline()).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"preserve");
    let other = tempfile::tempdir().unwrap();
    symlink(home.path().join("Library"), other.path().join("Library")).unwrap();
    assert!(managed(managed_path(other.path())).refresh().is_err());
}
#[test]
fn sparse_oversized_managed_database_is_rejected_before_reading() {
    let home = tempfile::tempdir().unwrap();
    let path = managed_path(home.path());
    install_fixture(&path);
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(database::MAX_DATABASE as u64 + 1)
        .unwrap();
    assert!(managed(path).refresh().is_err());
}

#[test]
fn unchanged_invalid_replacement_is_not_revalidated_and_keeps_good_lookups() {
    let home = tempfile::tempdir().unwrap();
    let path = managed_path(home.path());
    install_fixture(&path);
    let mut database = managed(path.clone());
    database.refresh().unwrap();
    let address = "89.160.20.112".parse().unwrap();
    let country = database.lookup(address);
    let good_stamp = database.stamp.clone();
    let cache = database.cache.clone();
    let order = database.order.clone();
    let description = database.description();

    fs::write(&path, b"not an MMDB").unwrap();
    let first_error = database.refresh().unwrap_err().to_string();
    assert_eq!(database.stamp, good_stamp);
    assert_eq!(database.cache, cache);
    assert_eq!(database.order, order);
    assert_eq!(database.description(), description);
    for _ in 0..3 {
        let repeated_error = database
            .refresh_with(|_| panic!("unchanged corrupt database was revalidated"))
            .unwrap_err()
            .to_string();
        assert_eq!(repeated_error, first_error);
    }
    assert_eq!(database.lookup(address), country);
    assert_eq!(database.cache, cache);

    // The updater's atomic replacement changes the stamp and must clear both
    // the cached failure and lookup cache after validating the new database.
    install_fixture(&path);
    let mut loaded = false;
    database
        .refresh_with(|path| {
            loaded = true;
            database::load(path)
        })
        .unwrap();
    assert!(loaded);
    assert!(database.failed_refresh.is_none());
    assert!(database.cache.is_empty());
    assert!(database.order.is_empty());
    assert_ne!(database.stamp, good_stamp);
    assert_eq!(database.lookup(address), country);
}

#[test]
fn initially_invalid_managed_database_recovers_when_its_file_changes() {
    let home = tempfile::tempdir().unwrap();
    let path = managed_path(home.path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"initially corrupt").unwrap();
    let mut database = managed(path.clone());
    assert!(database.refresh().is_err());
    assert!(database.description().is_none());
    assert_eq!(database.path.as_ref(), Some(&path));
    assert!(
        database
            .refresh_with(|_| panic!("initial failure was revalidated"))
            .is_err()
    );

    // A changed file is retried even if it is still invalid.
    fs::write(&path, b"a different invalid database").unwrap();
    let mut retried = false;
    assert!(
        database
            .refresh_with(|path| {
                retried = true;
                database::load(path)
            })
            .is_err()
    );
    assert!(retried);

    install_fixture(&path);
    database.refresh().unwrap();
    assert!(database.failed_refresh.is_none());
    assert_eq!(
        database
            .lookup("89.160.20.112".parse().unwrap())
            .unwrap()
            .code,
        "SE"
    );
}
