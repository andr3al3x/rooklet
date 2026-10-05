//! Offline country estimates from the managed DB-IP Country Lite database.
mod database;
mod download;
#[cfg(test)]
mod offline_tests;
#[cfg(test)]
mod tests;

use anyhow::{Result, anyhow};
use maxminddb::{Reader, geoip2};
use rooklet_core::model::Country;
use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    path::PathBuf,
    sync::atomic::AtomicBool,
};

const CACHE_LIMIT: usize = 4096;
#[derive(Default)]
pub(crate) struct GeoIp {
    reader: Option<Reader<Vec<u8>>>,
    path: Option<PathBuf>,
    stamp: Option<database::FileStamp>,
    failed_refresh: Option<FailedRefresh>,
    cache: HashMap<IpAddr, Option<Country>>,
    order: VecDeque<IpAddr>,
}
struct FailedRefresh {
    stamp: database::FileStamp,
    error: String,
}
impl GeoIp {
    /// Track the managed location without loading it. Call `refresh` to load or
    /// retry changed data while retaining the path even after an initial error.
    pub(crate) fn managed() -> Result<Self> {
        Ok(Self {
            path: Some(database::managed_path()?),
            ..Self::default()
        })
    }
    pub(crate) fn is_managed(&self) -> bool {
        self.path.is_some()
    }
    pub(crate) fn description(&self) -> Option<String> {
        self.reader
            .as_ref()
            .map(|reader| database::description(reader.metadata.build_epoch))
    }
    /// Reload changed managed data, preserving the old reader and lookup cache on errors.
    /// An unchanged failed file returns its cached error without another validation scan.
    pub(crate) fn refresh(&mut self) -> Result<()> {
        self.refresh_with(database::load)
    }
    fn refresh_with(
        &mut self,
        mut load: impl FnMut(&std::path::Path) -> Result<Reader<Vec<u8>>>,
    ) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let stamp = database::file_stamp(path)?;
        if let Some(failure) = &self.failed_refresh
            && stamp.as_ref() == Some(&failure.stamp)
        {
            return Err(anyhow!("{}", failure.error));
        }
        self.failed_refresh = None;
        if stamp == self.stamp {
            return Ok(());
        }
        let reader = match &stamp {
            Some(current_stamp) => match load(path) {
                Ok(reader) => Some(reader),
                Err(error) => {
                    let error = format!("{error:#}");
                    self.failed_refresh = Some(FailedRefresh {
                        stamp: current_stamp.clone(),
                        error: error.clone(),
                    });
                    return Err(anyhow!(error));
                }
            },
            None => None,
        };
        self.reader = reader;
        self.stamp = stamp;
        self.cache.clear();
        self.order.clear();
        Ok(())
    }
    pub(crate) fn lookup(&mut self, ip: IpAddr) -> Option<Country> {
        if is_local(ip) {
            return None;
        }
        if let Some(value) = self.cache.get(&ip) {
            return value.clone();
        }
        let value = self.reader.as_ref().and_then(|reader| {
            let record = reader.lookup(ip).ok()?.decode::<geoip2::Country>().ok()??;
            let code = record.country.iso_code?;
            Some(Country {
                code: code.into(),
                name: record.country.names.english.unwrap_or(code).into(),
            })
        });
        if self.cache.len() >= CACHE_LIMIT
            && let Some(old) = self.order.pop_front()
        {
            self.cache.remove(&old);
        }
        self.order.push_back(ip);
        self.cache.insert(ip, value.clone());
        value
    }
}

/// Fetch only the monthly provider archive. Observed endpoint addresses stay offline.
pub fn update(cancel: &AtomicBool) -> Result<String> {
    download::update(cancel)
}

pub(crate) fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_broadcast()
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return is_local(IpAddr::V4(v4));
            }
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}
