//! Short-lived control observations; explicit operations always bypass the cache.
use rooklet_core::model::Snapshot;
use std::time::{Duration, Instant};

const INTERVAL: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct Controls {
    reading: Option<(Instant, Snapshot)>,
}

impl Controls {
    pub(super) fn read(&mut self, force: bool, read: impl FnOnce() -> Snapshot) -> Snapshot {
        self.read_at(Instant::now(), force, read)
    }

    fn read_at(&mut self, now: Instant, force: bool, read: impl FnOnce() -> Snapshot) -> Snapshot {
        if force
            || self
                .reading
                .as_ref()
                .is_none_or(|(at, _)| now.duration_since(*at) >= INTERVAL)
        {
            self.reading = Some((now, read()));
        }
        let (at, reading) = self
            .reading
            .as_ref()
            .expect("control observation was initialized");
        let mut snapshot = reading.clone();
        snapshot.control_age_ms =
            Some(now.duration_since(*at).as_millis().min(u64::MAX as u128) as u64);
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periodic_reads_reuse_recent_controls_and_report_age() {
        let now = Instant::now();
        let mut cache = Controls::default();
        let first = cache.read_at(now, false, || Snapshot {
            firewall: Some(Default::default()),
            ..Default::default()
        });
        assert_eq!(first.control_age_ms, Some(0));
        let recent = cache.read_at(now + Duration::from_secs(2), false, || {
            panic!("redundant query")
        });
        assert_eq!(recent.control_age_ms, Some(2000));
        assert!(recent.firewall.is_some());
        let expired = cache.read_at(now + INTERVAL, false, Snapshot::default);
        assert_eq!(expired.control_age_ms, Some(0));
        assert!(expired.firewall.is_none());
    }

    #[test]
    fn explicit_read_does_not_preserve_previous_success_after_failure() {
        let now = Instant::now();
        let mut cache = Controls::default();
        cache.read_at(now, false, || Snapshot {
            firewall: Some(Default::default()),
            ..Default::default()
        });
        let failed = cache.read_at(now + Duration::from_millis(1), true, || Snapshot {
            notices: vec!["firewall unavailable".into()],
            ..Default::default()
        });
        assert!(failed.firewall.is_none());
        assert_eq!(failed.notices, ["firewall unavailable"]);
        assert_eq!(failed.control_age_ms, Some(0));
        assert!(
            cache
                .read_at(now + Duration::from_secs(1), false, || panic!(
                    "cached failure"
                ))
                .firewall
                .is_none()
        );
    }
}
