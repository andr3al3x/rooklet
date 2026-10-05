//! Bounded Activity queries and deterministic presentation ordering.
use super::selection::process_key;
use crate::{
    model::{Connection, ProcessActivity, Protocol},
    permissions::IncomingState,
};
use ipnet::IpNet;
use std::{cmp::Ordering, net::IpAddr};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ActivitySort {
    #[default]
    Snapshot,
    DownloadRate,
    UploadRate,
    DownloadTotal,
    UploadTotal,
    Name,
    Peers,
}
impl ActivitySort {
    pub fn next(self) -> Self {
        match self {
            Self::Snapshot => Self::DownloadRate,
            Self::DownloadRate => Self::UploadRate,
            Self::UploadRate => Self::DownloadTotal,
            Self::DownloadTotal => Self::UploadTotal,
            Self::UploadTotal => Self::Name,
            Self::Name => Self::Peers,
            Self::Peers => Self::Snapshot,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Snapshot => "Observed order",
            Self::DownloadRate => "Download rate",
            Self::UploadRate => "Upload rate",
            Self::DownloadTotal => "Download total",
            Self::UploadTotal => "Upload total",
            Self::Name => "Name",
            Self::Peers => "Peers",
        }
    }
    pub(super) fn compare(self, a: &ProcessActivity, b: &ProcessActivity) -> Ordering {
        let order = match self {
            Self::Snapshot => return Ordering::Equal,
            Self::DownloadRate => b.rate_in.cmp(&a.rate_in),
            Self::UploadRate => b.rate_out.cmp(&a.rate_out),
            Self::DownloadTotal => b.bytes_in.cmp(&a.bytes_in),
            Self::UploadTotal => b.bytes_out.cmp(&a.bytes_out),
            Self::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Self::Peers => b.connections.len().cmp(&a.connections.len()),
        };
        order.then_with(|| process_key(a).cmp(&process_key(b)))
    }
}

enum Predicate {
    App(String),
    Country(String),
    Protocol(Protocol),
    Incoming(IncomingState),
    Local(bool),
    Ip(IpNet),
    Port(u16),
}
#[derive(Default)]
pub(super) struct ActivityQuery {
    plain: String,
    predicates: Vec<Predicate>,
}
impl ActivityQuery {
    pub(super) fn parse(input: &str) -> Result<Self, String> {
        if input.chars().count() > 256 {
            return Err("Activity filter exceeds 256 characters".into());
        }
        let mut query = Self::default();
        let mut words = Vec::new();
        let mut word = String::new();
        let mut quote = false;
        for ch in input.chars() {
            match ch {
                '"' => quote = !quote,
                ch if ch.is_whitespace() && !quote => {
                    if !word.is_empty() {
                        words.push(std::mem::take(&mut word));
                    }
                }
                ch => word.push(ch),
            }
        }
        if quote {
            return Err("Unclosed quote in Activity filter".into());
        }
        if !word.is_empty() {
            words.push(word);
        }
        let mut plain = Vec::new();
        for word in words {
            // An unprefixed IPv6 literal remains an ordinary substring search.
            if word.parse::<IpAddr>().is_ok() || !word.contains(':') {
                plain.push(word.to_lowercase());
                continue;
            }
            let (key, value) = word.split_once(':').expect("colon checked");
            let value = value.to_lowercase();
            if value.is_empty() {
                return Err(format!("Missing value for {key}:"));
            }
            let invalid = || format!("Invalid {key}: value {value}");
            let predicate = match key.to_lowercase().as_str() {
                "app" => Predicate::App(value),
                "country" => Predicate::Country(value),
                "proto" => Predicate::Protocol(match value.as_str() {
                    "tcp" => Protocol::Tcp,
                    "udp" => Protocol::Udp,
                    "any" => Protocol::Any,
                    _ => return Err(invalid()),
                }),
                "incoming" => Predicate::Incoming(match value.as_str() {
                    "allow" => IncomingState::Allow,
                    "block" => IncomingState::Block,
                    "mixed" => IncomingState::Mixed,
                    "unregistered" => IncomingState::Unlisted,
                    "unavailable" => IncomingState::Unknown,
                    _ => return Err(invalid()),
                }),
                "scope" if matches!(value.as_str(), "local" | "public") => {
                    Predicate::Local(value == "local")
                }
                "ip" => Predicate::Ip(
                    value
                        .parse::<IpNet>()
                        .or_else(|_| value.parse::<IpAddr>().map(IpNet::from))
                        .map_err(|_| invalid())?,
                ),
                "port" => Predicate::Port(
                    value
                        .parse::<u16>()
                        .ok()
                        .filter(|port| *port != 0)
                        .ok_or_else(invalid)?,
                ),
                "scope" => return Err(invalid()),
                _ => return Err(format!("Unknown Activity filter field {key}:")),
            };
            query.predicates.push(predicate);
        }
        query.plain = plain.join(" ");
        Ok(query)
    }
    pub(super) fn needs_peer(&self) -> bool {
        self.predicates.iter().any(|p| {
            matches!(
                p,
                Predicate::Country(_)
                    | Predicate::Protocol(_)
                    | Predicate::Local(_)
                    | Predicate::Ip(_)
                    | Predicate::Port(_)
            )
        })
    }
    pub(super) fn needs_incoming(&self) -> bool {
        self.predicates
            .iter()
            .any(|predicate| matches!(predicate, Predicate::Incoming(_)))
    }
    pub(super) fn process_matches(&self, process: &ProcessActivity, state: IncomingState) -> bool {
        self.predicates.iter().all(|predicate| match predicate {
            Predicate::App(text) => {
                format!("{} {}", process.name, process.path.as_deref().unwrap_or(""))
                    .to_lowercase()
                    .contains(text)
            }
            Predicate::Incoming(expected) => state == *expected,
            _ => true,
        })
    }
    pub(super) fn plain_process_matches(&self, process: &ProcessActivity) -> bool {
        self.plain.is_empty() || process_text(process).contains(&self.plain)
    }
    pub(super) fn flow_matches(&self, process: &ProcessActivity, flow: &Connection) -> bool {
        let plain_matches = self.plain.is_empty() || {
            let country = if flow.local {
                "local network".into()
            } else {
                flow.country
                    .as_ref()
                    .map(|c| format!("{} {}", c.code, c.name))
                    .unwrap_or_else(|| "unknown".into())
            };
            let text = format!(
                "{} {} {} {} {}",
                process_text(process),
                flow.remote_ip,
                flow.remote_port.map(|p| p.to_string()).unwrap_or_default(),
                flow.protocol,
                country
            )
            .to_lowercase();
            text.contains(&self.plain)
        };
        plain_matches
            && self.predicates.iter().all(|predicate| match predicate {
                Predicate::Country(value) => match value.as_str() {
                    "local" => flow.local,
                    "unknown" => !flow.local && flow.country.is_none(),
                    _ => {
                        !flow.local
                            && flow.country.as_ref().is_some_and(|c| {
                                c.code.eq_ignore_ascii_case(value)
                                    || c.name.eq_ignore_ascii_case(value)
                            })
                    }
                },
                Predicate::Protocol(value) => *value == Protocol::Any || flow.protocol == *value,
                Predicate::Local(local) => flow.local == *local,
                Predicate::Ip(network) => flow
                    .remote_ip
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| network.contains(&ip)),
                Predicate::Port(port) => flow.remote_port == Some(*port),
                _ => true,
            })
    }
}
fn process_text(process: &ProcessActivity) -> String {
    format!(
        "{} {} {}",
        process.name,
        process.pid,
        process.path.as_deref().unwrap_or("")
    )
    .to_lowercase()
}
