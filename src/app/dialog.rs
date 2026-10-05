//! Typed dialogs and editable network-rule drafts.
use crate::model::{Action, Direction, Mutation, NetworkRule, Protocol};

#[derive(Debug, Clone)]
pub enum ConfirmedAction {
    Firewall(Mutation),
    Terminate(crate::process::TerminationRequest),
    Profile(Box<crate::profile::Prepared>),
}

#[derive(Debug, Clone)]
pub enum Popup {
    Help,
    Inspect(String),
    Profiles {
        entries: Vec<String>,
        selected: usize,
        loading: bool,
    },
    ProfileName {
        name: String,
    },
    Confirm {
        title: String,
        body: String,
        /// Normalized by the renderer to the actual wrapped content and viewport.
        scroll: std::cell::Cell<u16>,
        action: ConfirmedAction,
    },
    Application {
        path: String,
    },
    Network {
        draft: NetworkDraft,
        field: usize,
    },
    Explain {
        draft: super::RuleProbe,
        field: usize,
    },
}
#[derive(Debug, Clone)]
pub struct NetworkDraft {
    pub enabled: bool,
    pub id: String,
    pub name: String,
    pub destination: String,
    pub port: String,
    pub protocol: Protocol,
    pub direction: Direction,
    pub action: Action,
    pub interface: String,
}
impl NetworkDraft {
    pub(super) fn from_peer(flow: &crate::model::Connection) -> Self {
        let mut draft = Self::new(flow.remote_ip.clone());
        draft.name = format!("Peer {}", flow.remote_ip);
        draft.protocol = flow.protocol;
        if flow.protocol != Protocol::Any {
            draft.port = flow
                .remote_port
                .map(|port| port.to_string())
                .unwrap_or_default();
        }
        // Direction is a proposed policy, not inferred from observed traffic.
        draft.direction = Direction::Outbound;
        draft
    }
    pub(super) fn new(destination: String) -> Self {
        let serial = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            enabled: true,
            id: format!("rule-{serial}"),
            name: "Network rule".into(),
            destination,
            port: String::new(),
            protocol: Protocol::Any,
            direction: Direction::Outbound,
            action: Action::Block,
            interface: String::new(),
        }
    }
    pub(super) fn from_rule(rule: &NetworkRule) -> Self {
        Self {
            enabled: rule.enabled,
            id: rule.id.clone(),
            name: rule.name.clone(),
            destination: rule.destination.clone(),
            port: rule.port.map(|p| p.to_string()).unwrap_or_default(),
            protocol: rule.protocol,
            direction: rule.direction,
            action: rule.action,
            interface: rule.interface.clone().unwrap_or_default(),
        }
    }
    pub(super) fn rule(&self) -> anyhow::Result<NetworkRule> {
        let rule = NetworkRule {
            id: self.id.clone(),
            name: self.name.trim().into(),
            action: self.action,
            destination: self.destination.trim().into(),
            port: if self.port.trim().is_empty() {
                None
            } else {
                Some(
                    self.port
                        .trim()
                        .parse()
                        .map_err(|_| anyhow::anyhow!("Port must be 1–65535"))?,
                )
            },
            protocol: self.protocol,
            direction: self.direction,
            interface: if self.interface.trim().is_empty() {
                None
            } else {
                Some(self.interface.trim().into())
            },
            enabled: self.enabled,
        };
        crate::network::validate_rules(std::slice::from_ref(&rule))?;
        Ok(rule)
    }
    pub(super) fn text_mut(&mut self, field: usize) -> Option<&mut String> {
        match field {
            0 => Some(&mut self.destination),
            1 => Some(&mut self.port),
            5 => Some(&mut self.interface),
            6 => Some(&mut self.name),
            _ => None,
        }
    }
    pub(super) fn cycle(&mut self, field: usize) {
        match field {
            2 => {
                self.protocol = match self.protocol {
                    Protocol::Any => Protocol::Tcp,
                    Protocol::Tcp => Protocol::Udp,
                    Protocol::Udp => Protocol::Any,
                }
            }
            3 => {
                self.direction = match self.direction {
                    Direction::Outbound => Direction::Inbound,
                    Direction::Inbound => Direction::Both,
                    Direction::Both => Direction::Outbound,
                }
            }
            4 => {
                self.action = if self.action == Action::Allow {
                    Action::Block
                } else {
                    Action::Allow
                }
            }
            _ => {}
        }
    }
}
