//! nettop CSV sample boundaries, raw counters, and socket endpoints.
use super::{MAX_LINE, MAX_RECORDS};
use crate::model::Protocol;
use anyhow::{Context, Result, bail, ensure};
use std::net::IpAddr;

#[derive(Debug, Clone)]
pub struct RawFlow {
    pub key: String,
    pub remote: IpAddr,
    pub port: Option<u16>,
    pub protocol: Protocol,
    pub bytes_in: u64,
    pub bytes_out: u64,
}
#[derive(Debug, Clone)]
pub struct RawProcess {
    pub pid: u32,
    pub name: String,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub flows: Vec<RawFlow>,
}
#[derive(Default)]
pub struct CsvParser {
    columns: Option<(usize, usize)>,
    current: Option<usize>,
    processes: Vec<RawProcess>,
    records: usize,
}
impl CsvParser {
    /// A repeated column header completes the previous sample. Empty socket counters
    /// and wildcard peers are deliberately excluded from destination attribution.
    pub fn push(&mut self, line: &[u8]) -> Result<Option<Vec<RawProcess>>> {
        ensure!(line.len() <= MAX_LINE, "nettop CSV record exceeds 64 KiB");
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(line);
        let Some(record) = reader.records().next() else {
            return Ok(None);
        };
        let record = record.context("invalid nettop CSV record")?;
        if record.is_empty() {
            return Ok(None);
        }
        if let Some(input) = record.iter().position(|v| v == "bytes_in") {
            let output = record
                .iter()
                .position(|v| v == "bytes_out")
                .context("nettop header lacks bytes_out")?;
            ensure!(input > 0 && output > 0, "unsupported nettop column header");
            let completed = self.columns.map(|_| std::mem::take(&mut self.processes));
            self.columns = Some((input, output));
            self.current = None;
            self.records = 0;
            return Ok(completed);
        }
        let (incoming, outgoing) = self.columns.context("nettop CSV lacks column header")?;
        let label = record.get(0).unwrap_or("").trim();
        if label.is_empty() || label == "time" {
            return Ok(None);
        }
        self.records += 1;
        ensure!(
            self.records <= MAX_RECORDS,
            "nettop sample exceeds 32768 records"
        );
        let bytes = || -> Result<Option<(u64, u64)>> {
            let a = record
                .get(incoming)
                .context("nettop record lacks bytes_in")?;
            let b = record
                .get(outgoing)
                .context("nettop record lacks bytes_out")?;
            if a.is_empty() && b.is_empty() {
                return Ok(None);
            }
            Ok(Some((
                a.parse().context("invalid nettop bytes_in")?,
                b.parse().context("invalid nettop bytes_out")?,
            )))
        };
        if ["tcp4 ", "tcp6 ", "udp4 ", "udp6 ", "quic4 ", "quic6 "]
            .iter()
            .any(|prefix| label.starts_with(prefix))
        {
            let Some(index) = self.current else {
                bail!("nettop socket has no process");
            };
            if let Some((a, b)) = bytes()?
                && let Some((remote, port, protocol)) = parse_flow(label)
            {
                self.processes[index].flows.push(RawFlow {
                    key: label.into(),
                    remote,
                    port,
                    protocol,
                    bytes_in: a,
                    bytes_out: b,
                });
            }
        } else {
            let (name, pid) = label
                .rsplit_once('.')
                .context("invalid nettop process identity")?;
            let pid: u32 = pid.parse().context("invalid nettop process PID")?;
            ensure!(
                pid > 0 && !name.is_empty(),
                "invalid nettop process identity"
            );
            let (a, b) = bytes()?.context("nettop process lacks counters")?;
            self.current = Some(self.processes.len());
            self.processes.push(RawProcess {
                pid,
                name: name.into(),
                bytes_in: a,
                bytes_out: b,
                flows: Vec::new(),
            });
        }
        Ok(None)
    }
    pub fn finish(&mut self) -> Vec<RawProcess> {
        self.current = None;
        std::mem::take(&mut self.processes)
    }
}
pub fn parse_csv(input: &str) -> Result<Vec<Vec<RawProcess>>> {
    let mut parser = CsvParser::default();
    let mut samples = Vec::new();
    for line in input.lines() {
        if let Some(sample) = parser.push(line.as_bytes())? {
            samples.push(sample);
        }
    }
    let final_sample = parser.finish();
    if !final_sample.is_empty() {
        samples.push(final_sample);
    }
    Ok(samples)
}
pub fn parse_flow(label: &str) -> Option<(IpAddr, Option<u16>, Protocol)> {
    let (kind, endpoints) = label.split_once(' ')?;
    let protocol = match kind {
        "tcp4" | "tcp6" => Protocol::Tcp,
        "udp4" | "udp6" | "quic4" | "quic6" => Protocol::Udp,
        _ => return None,
    };
    let (_, remote) = endpoints.split_once("<->")?;
    let (address, port) = if let Some(bracketed) = remote.strip_prefix('[') {
        let (address, suffix) = bracketed.split_once(']')?;
        (address, suffix.strip_prefix(':')?)
    } else if kind.ends_with('6') {
        remote.rsplit_once('.')?
    } else {
        remote.rsplit_once(':')?
    };
    let address = address.split('%').next()?.parse().ok()?;
    let port = if port == "*" {
        None
    } else {
        Some(port.parse().ok()?)
    };
    Some((address, port, protocol))
}
