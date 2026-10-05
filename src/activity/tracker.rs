//! Session counter baselines, application totals, and peer aggregation.
use super::{
    MAX_RECORDS,
    identity::{group_identity, process_path},
    parser::RawProcess,
};
use crate::{
    geoip::{GeoIp, is_local},
    model::{Connection, ProcessActivity},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::Duration,
};

struct CounterReading {
    bytes_in: u64,
    bytes_out: u64,
}

#[derive(Default)]
struct Counter {
    previous: Option<(u64, u64, Duration)>,
    total: (u64, u64),
}
impl Counter {
    fn update(&mut self, incoming: u64, outgoing: u64, elapsed: Duration) -> CounterReading {
        if let Some((previous_in, previous_out, _)) = self.previous {
            // Counter resets establish a fresh baseline without creating a wraparound spike.
            let delta_in = incoming.saturating_sub(previous_in);
            let delta_out = outgoing.saturating_sub(previous_out);
            self.total.0 = self.total.0.saturating_add(delta_in);
            self.total.1 = self.total.1.saturating_add(delta_out);
        }
        self.previous = Some((incoming, outgoing, elapsed));
        CounterReading {
            bytes_in: self.total.0,
            bytes_out: self.total.1,
        }
    }
}
#[derive(Default)]
struct GroupTotal {
    incoming: u64,
    outgoing: u64,
    last_seen: Duration,
    rate_baseline: Option<(u64, u64, Duration)>,
}
impl GroupTotal {
    fn rates(&mut self, elapsed: Duration) -> (u64, u64) {
        let (incoming, outgoing, at) = *self.rate_baseline.get_or_insert((0, 0, elapsed));
        let seconds = elapsed.saturating_sub(at).as_secs_f64();
        if seconds < 0.05 {
            return (0, 0);
        }
        (
            (self.incoming.saturating_sub(incoming) as f64 / seconds) as u64,
            (self.outgoing.saturating_sub(outgoing) as f64 / seconds) as u64,
        )
    }
    fn finish_batch(&mut self, elapsed: Duration) {
        if self.last_seen == elapsed
            && self.rate_baseline.is_some_and(|(_, _, at)| {
                at == elapsed || elapsed.saturating_sub(at) >= Duration::from_millis(50)
            })
        {
            self.rate_baseline = Some((self.incoming, self.outgoing, elapsed));
        }
    }
}
#[derive(Default)]
pub struct Tracker {
    counters: HashMap<String, Counter>,
    groups: HashMap<String, GroupTotal>,
}
impl Tracker {
    pub fn update(
        &mut self,
        sample: Vec<RawProcess>,
        elapsed: Duration,
        geoip: &mut GeoIp,
    ) -> Vec<ProcessActivity> {
        self.update_with_paths(sample, elapsed, geoip, process_path)
    }
    /// Buffered samples share one observation interval. Keep every counter transition
    /// for session totals, then advance rate baselines after the entire read batch.
    pub fn update_batch(
        &mut self,
        samples: Vec<Vec<RawProcess>>,
        elapsed: Duration,
        geoip: &mut GeoIp,
    ) -> Vec<ProcessActivity> {
        self.update_batch_with_paths(samples, elapsed, geoip, process_path)
    }
    /// Path resolution is separate so buffered helper exits can use verified bundle fixtures.
    pub fn update_batch_with_paths(
        &mut self,
        samples: Vec<Vec<RawProcess>>,
        elapsed: Duration,
        geoip: &mut GeoIp,
        mut resolve: impl FnMut(u32) -> Option<String>,
    ) -> Vec<ProcessActivity> {
        let mut latest = Vec::new();
        for sample in samples {
            latest = self.ingest(sample, elapsed, geoip, &mut resolve);
        }
        self.finish_batch(elapsed);
        latest
    }
    /// Path resolution is separate so tests can use verified bundle fixtures.
    pub fn update_with_paths(
        &mut self,
        sample: Vec<RawProcess>,
        elapsed: Duration,
        geoip: &mut GeoIp,
        mut resolve: impl FnMut(u32) -> Option<String>,
    ) -> Vec<ProcessActivity> {
        let latest = self.ingest(sample, elapsed, geoip, &mut resolve);
        self.finish_batch(elapsed);
        latest
    }
    fn finish_batch(&mut self, elapsed: Duration) {
        for group in self.groups.values_mut() {
            group.finish_batch(elapsed);
        }
    }
    fn ingest(
        &mut self,
        sample: Vec<RawProcess>,
        elapsed: Duration,
        geoip: &mut GeoIp,
        mut resolve: impl FnMut(u32) -> Option<String>,
    ) -> Vec<ProcessActivity> {
        let mut seen = HashSet::new();
        let mut grouped = BTreeMap::<String, ProcessActivity>::new();
        for process in sample.into_iter().take(MAX_RECORDS) {
            let path = resolve(process.pid);
            let identity = format!(
                "{}:{}:{}",
                process.pid,
                process.name,
                path.as_deref().unwrap_or("")
            );
            if !seen.insert(identity.clone()) {
                continue;
            }
            let previous_total = self
                .counters
                .get(&identity)
                .map_or((0, 0), |counter| counter.total);
            let values = self.counters.entry(identity.clone()).or_default().update(
                process.bytes_in,
                process.bytes_out,
                elapsed,
            );
            let (key, name, app_path) = group_identity(process.pid, &process.name, path);
            if !self.groups.contains_key(&key)
                && self.groups.len() >= MAX_RECORDS
                && let Some(oldest) = self
                    .groups
                    .iter()
                    .min_by_key(|(_, value)| value.last_seen)
                    .map(|(key, _)| key.clone())
            {
                self.groups.remove(&oldest);
            }
            let total = self.groups.entry(key.clone()).or_default();
            total.incoming = total
                .incoming
                .saturating_add(values.bytes_in.saturating_sub(previous_total.0));
            total.outgoing = total
                .outgoing
                .saturating_add(values.bytes_out.saturating_sub(previous_total.1));
            total.last_seen = elapsed;
            let group = grouped.entry(key).or_insert_with(|| ProcessActivity {
                pid: process.pid,
                name,
                path: app_path,
                identities: Vec::new(),
                bytes_in: 0,
                bytes_out: 0,
                rate_in: 0,
                rate_out: 0,
                connections: Vec::new(),
            });
            group.pid = group.pid.min(process.pid);
            group.bytes_in = total.incoming;
            group.bytes_out = total.outgoing;
            (group.rate_in, group.rate_out) = total.rates(elapsed);
            for flow in process.flows {
                let flow_key = format!("{identity}|{}", flow.key);
                if !seen.insert(flow_key.clone()) {
                    continue;
                }
                if self.counters.len() >= MAX_RECORDS && !self.counters.contains_key(&flow_key) {
                    continue;
                }
                let counters = self.counters.entry(flow_key).or_default().update(
                    flow.bytes_in,
                    flow.bytes_out,
                    elapsed,
                );
                group.connections.push(Connection {
                    remote_ip: flow.remote.to_string(),
                    remote_port: flow.port,
                    protocol: flow.protocol,
                    bytes_in: counters.bytes_in,
                    bytes_out: counters.bytes_out,
                    country: geoip.lookup(flow.remote),
                    local: is_local(flow.remote),
                });
            }
        }
        // Keep baselines for recently closed processes/flows, but bound observation memory.
        self.counters.retain(|_, value| {
            value
                .previous
                .is_some_and(|(_, _, at)| elapsed.saturating_sub(at) < Duration::from_secs(60))
        });
        if self.counters.len() > MAX_RECORDS {
            self.counters.retain(|key, _| seen.contains(key));
        }
        grouped
            .into_values()
            .map(|mut group| {
                group.connections = merge_peers(std::mem::take(&mut group.connections));
                group
            })
            .collect()
    }
}

fn merge_peers(connections: Vec<Connection>) -> Vec<Connection> {
    let mut peers = BTreeMap::<(String, Option<u16>, String), Connection>::new();
    for connection in connections {
        let key = (
            connection.remote_ip.clone(),
            connection.remote_port,
            connection.protocol.to_string(),
        );
        if let Some(peer) = peers.get_mut(&key) {
            peer.bytes_in = peer.bytes_in.saturating_add(connection.bytes_in);
            peer.bytes_out = peer.bytes_out.saturating_add(connection.bytes_out);
        } else {
            peers.insert(key, connection);
        }
    }
    peers.into_values().collect()
}
