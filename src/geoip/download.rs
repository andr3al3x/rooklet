//! Fixed monthly provider downloads, bounded gzip decoding, and update orchestration.
use super::database;
use anyhow::{Context, Result, bail, ensure};
use flate2::bufread::GzDecoder;
use std::{
    io::Read,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const MAX_ARCHIVE: usize = 32 * 1024 * 1024;
const UPDATE_TIMEOUT: Duration = Duration::from_secs(60);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Month {
    pub(super) year: i64,
    pub(super) month: u32,
}
impl Month {
    fn current() -> Self {
        Self::from_date(time::OffsetDateTime::now_utc())
    }
    fn from_date(date: time::OffsetDateTime) -> Self {
        Self {
            year: i64::from(date.year()),
            month: u32::from(u8::from(date.month())),
        }
    }
    fn previous(self) -> Self {
        if self.month == 1 {
            Self {
                year: self.year - 1,
                month: 12,
            }
        } else {
            Self {
                year: self.year,
                month: self.month - 1,
            }
        }
    }
    fn url(self) -> String {
        format!(
            "https://download.db-ip.com/free/dbip-country-lite-{}-{:02}.mmdb.gz",
            self.year, self.month
        )
    }
}
pub(super) enum Download {
    NotFound,
    Archive(Vec<u8>),
}
pub(super) fn check(cancel: &AtomicBool, deadline: Instant) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "GeoIP update cancelled");
    ensure!(Instant::now() < deadline, "GeoIP update timed out");
    Ok(())
}
fn fetch(url: &str, cancel: &AtomicBool, deadline: Instant) -> Result<Download> {
    check(cancel, deadline)?;
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .http_status_as_error(false)
        .proxy(None)
        .user_agent("xield/0.2 DB-IP Country Lite updater")
        .timeout_global(Some(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_secs(30)),
        ))
        .timeout_resolve(Some(Duration::from_secs(5)))
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_recv_response(Some(Duration::from_secs(5)))
        .timeout_recv_body(Some(Duration::from_secs(25)))
        .max_response_header_size(16 * 1024)
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get(url)
        .call()
        .context("cannot download DB-IP Country Lite over HTTPS")?;
    check(cancel, deadline)?;
    if response.status().as_u16() == 404 {
        return Ok(Download::NotFound);
    }
    ensure!(
        response.status().as_u16() == 200,
        "DB-IP download returned HTTP {}",
        response.status().as_u16()
    );
    if let Some(length) = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
    {
        let length: usize = length.parse().context("invalid DB-IP download length")?;
        ensure!(length <= MAX_ARCHIVE, "DB-IP download exceeds 32 MiB");
    }
    let bytes = read_bounded(
        response.body_mut().as_reader(),
        MAX_ARCHIVE,
        cancel,
        deadline,
    )?;
    Ok(Download::Archive(bytes))
}
pub(super) fn read_bounded(
    mut source: impl Read,
    limit: usize,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        check(cancel, deadline)?;
        let count = source
            .read(&mut chunk)
            .context("cannot read DB-IP download")?;
        if count == 0 {
            break;
        }
        ensure!(
            bytes.len().saturating_add(count) <= limit,
            "DB-IP data exceeds its size limit"
        );
        bytes.extend_from_slice(&chunk[..count]);
    }
    check(cancel, deadline)?;
    Ok(bytes)
}
pub(super) fn decode(archive: &[u8], cancel: &AtomicBool, deadline: Instant) -> Result<Vec<u8>> {
    ensure!(
        archive.len() <= MAX_ARCHIVE,
        "DB-IP download exceeds 32 MiB"
    );
    let mut decoder = GzDecoder::new(archive);
    let bytes = read_bounded(&mut decoder, database::MAX_DATABASE, cancel, deadline)
        .context("invalid or truncated DB-IP gzip archive")?;
    ensure!(
        decoder.into_inner().is_empty(),
        "DB-IP gzip archive has unexpected trailing data"
    );
    Ok(bytes)
}
pub(super) fn update(cancel: &AtomicBool) -> Result<String> {
    let path = database::managed_path()?;
    update_with(&path, Month::current(), cancel, fetch)
}
pub(super) fn update_with(
    path: &Path,
    month: Month,
    cancel: &AtomicBool,
    mut fetch: impl FnMut(&str, &AtomicBool, Instant) -> Result<Download>,
) -> Result<String> {
    let deadline = Instant::now() + UPDATE_TIMEOUT;
    check(cancel, deadline)?;
    database::file_stamp(path)?;
    let archive = match fetch(&month.url(), cancel, deadline)? {
        Download::Archive(archive) => archive,
        Download::NotFound => match fetch(&month.previous().url(), cancel, deadline)? {
            Download::Archive(archive) => archive,
            Download::NotFound => {
                bail!("DB-IP Country Lite is unavailable for this month and the previous month")
            }
        },
    };
    let bytes = decode(&archive, cancel, deadline)?;
    let reader = database::validate_source(&bytes, cancel, deadline)?;
    let description = database::description(reader.metadata.build_epoch);
    database::install(path, &bytes, cancel, deadline)?;
    Ok(format!(
        "Installed {description} at {}\nIP Geolocation by DB-IP.com: https://db-ip.com · License: https://creativecommons.org/licenses/by/4.0/",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utc_months_and_year_boundary() {
        let date = time::Date::from_calendar_date(2026, time::Month::January, 1)
            .unwrap()
            .midnight()
            .assume_utc();
        let month = Month::from_date(date);
        assert_eq!(
            month,
            Month {
                year: 2026,
                month: 1
            }
        );
        assert_eq!(
            month.previous(),
            Month {
                year: 2025,
                month: 12
            }
        );
        let leap = time::Date::from_calendar_date(2024, time::Month::February, 29)
            .unwrap()
            .midnight()
            .assume_utc();
        assert_eq!(
            Month::from_date(leap),
            Month {
                year: 2024,
                month: 2
            }
        );
    }
}
