//! Hypothetical connection editing and review of ordered machine-wide policy.
use super::{App, clean};
use crate::{
    model::{Direction, Mutation, Protocol},
    network::{self, RuleQuery},
};

#[derive(Debug, Clone)]
pub struct RuleProbe {
    pub remote: String,
    pub port: String,
    pub protocol: Protocol,
    pub direction: Direction,
    pub interface: String,
}

impl Default for RuleProbe {
    fn default() -> Self {
        Self {
            remote: String::new(),
            port: String::new(),
            protocol: Protocol::Tcp,
            direction: Direction::Outbound,
            interface: String::new(),
        }
    }
}

impl RuleProbe {
    pub fn query(&self) -> anyhow::Result<RuleQuery> {
        Ok(RuleQuery {
            remote_ip: self
                .remote
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("Enter a remote IPv4 or IPv6 address."))?,
            destination_port: if self.port.trim().is_empty() {
                None
            } else {
                Some(
                    self.port
                        .trim()
                        .parse()
                        .map_err(|_| anyhow::anyhow!("Destination port must be 1–65535."))?,
                )
            },
            protocol: self.protocol,
            direction: self.direction,
            interface: if self.interface.trim().is_empty() {
                None
            } else {
                Some(self.interface.trim().into())
            },
        })
    }

    pub(super) fn text_mut(&mut self, field: usize) -> Option<&mut String> {
        match field {
            0 => Some(&mut self.remote),
            1 => Some(&mut self.port),
            4 => Some(&mut self.interface),
            _ => None,
        }
    }

    pub(super) fn cycle(&mut self, field: usize) {
        match field {
            2 => {
                self.protocol = if self.protocol == Protocol::Tcp {
                    Protocol::Udp
                } else {
                    Protocol::Tcp
                }
            }
            3 => {
                self.direction = if self.direction == Direction::Outbound {
                    Direction::Inbound
                } else {
                    Direction::Outbound
                }
            }
            _ => {}
        }
    }
}

impl App {
    pub(super) fn review_network_rules(&mut self, rules: Vec<crate::model::NetworkRule>) {
        let warnings = match network::shadow_warnings(&rules) {
            Ok(warnings) => warnings,
            Err(error) => {
                self.notify(error.to_string(), true);
                return;
            }
        };
        let mut body = String::from(
            "Applies to ALL applications on this Mac.\nFirst matching enabled Rooklet rule wins.\nExisting connections may continue through PF state.\n\n",
        );
        if !warnings.is_empty() {
            body.push_str("Shadow warnings (single earlier covering rule):\n");
            for warning in warnings {
                body.push_str(&format!(
                    "#{} {} is fully shadowed by #{} {}.\n",
                    warning.shadowed_position,
                    clean(&warning.shadowed_id),
                    warning.covering_position,
                    clean(&warning.covering_id)
                ));
            }
            body.push('\n');
        }
        body.push_str("Proposed ordered rules:\n");
        for (index, rule) in rules.iter().enumerate() {
            body.push_str(&format!(
                "{}. {} {} {} · {} {} · port {} · interface {}\n",
                index + 1,
                if rule.enabled { "on" } else { "off" },
                rule.action,
                clean(&rule.destination),
                rule.protocol,
                rule.direction,
                rule.port
                    .map(|port| port.to_string())
                    .unwrap_or_else(|| "any".into()),
                clean(rule.interface.as_deref().unwrap_or("any"))
            ));
        }
        if rules.is_empty() {
            body.push_str("No Rooklet rules remain.\n");
        }
        self.confirm(
            "Review machine-wide rules",
            body,
            Mutation::NetworkRules(rules),
        );
    }
}
