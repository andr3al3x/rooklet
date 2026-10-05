//! Pure validation and ordered PF rule compilation.
use crate::model::{Action, Direction, NetworkRule, Protocol};
use anyhow::{Context, Result, ensure};
use ipnet::IpNet;
use std::collections::HashSet;

/// macOS PF interface names occupy 16 bytes including their terminating NUL.
const MACOS_PF_INTERFACE_NAME_BYTES: usize = 16;

pub fn validate_rules(rules: &[NetworkRule]) -> Result<()> {
    ensure!(
        rules.len() <= 1000,
        "At most 1000 network rules are supported"
    );
    let mut ids = HashSet::new();
    for rule in rules {
        ensure!(
            !rule.id.is_empty()
                && rule.id.len() <= 55
                && rule
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
            "Rule ID must contain only letters, digits, underscores or hyphens (1–55 characters, fitting PF's 63-byte label limit)"
        );
        ensure!(ids.insert(&rule.id), "Duplicate rule ID: {}", rule.id);
        ensure!(
            !rule.name.trim().is_empty()
                && rule.name.len() <= 128
                && !rule.name.chars().any(char::is_control),
            "Rule {} has an invalid name",
            rule.id
        );
        destination(&rule.destination)
            .with_context(|| format!("Invalid destination for rule {}", rule.id))?;
        if let Some(port) = rule.port {
            ensure!(
                port != 0,
                "Rule {}: port must be between 1 and 65535",
                rule.id
            );
            ensure!(
                rule.protocol != Protocol::Any,
                "Rule {}: a port requires TCP or UDP",
                rule.id
            );
        }
        if let Some(interface) = &rule.interface {
            ensure!(
                valid_interface(interface),
                "Rule {}: invalid interface name",
                rule.id
            );
        }
    }
    Ok(())
}
pub(super) fn valid_interface(value: &str) -> bool {
    !value.is_empty()
        && value.len() < MACOS_PF_INTERFACE_NAME_BYTES
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        && value.as_bytes()[0].is_ascii_alphabetic()
}
pub(super) fn destination(value: &str) -> Result<Option<IpNet>> {
    if value == "any" {
        return Ok(None);
    }
    if let Ok(address) = value.parse::<std::net::IpAddr>() {
        return Ok(Some(IpNet::from(address)));
    }
    Ok(Some(
        value
            .parse::<IpNet>()
            .context("Use an IPv4/IPv6 address, CIDR, or 'any'")?
            .trunc(),
    ))
}
/// Compile ordered network rules. Every enabled rule is `quick`: first match wins.
/// Destination is the remote peer (inbound source / outbound destination).
/// Port is always the destination port (inbound local service / outbound remote service).
pub fn compile_rules(rules: &[NetworkRule]) -> Result<String> {
    validate_rules(rules)?;
    let mut output = String::from(
        "# Rooklet network rules: ordered, first match wins. Existing PF states are retained.\n",
    );
    for rule in rules.iter().filter(|rule| rule.enabled) {
        let network = destination(&rule.destination)?;
        let families: &[&str] = match network {
            Some(IpNet::V4(_)) => &["inet"],
            Some(IpNet::V6(_)) => &["inet6"],
            None => &["inet", "inet6"],
        };
        let directions: &[&str] = match rule.direction {
            Direction::Both => &["in", "out"],
            Direction::Inbound => &["in"],
            Direction::Outbound => &["out"],
        };
        let endpoint = network
            .map(|n| n.to_string())
            .unwrap_or_else(|| "any".into());
        for family in families {
            for direction in directions {
                output.push_str(match rule.action {
                    Action::Allow => "pass",
                    Action::Block => "block drop",
                });
                output.push_str(&format!(" {direction} quick"));
                if let Some(interface) = &rule.interface {
                    output.push_str(&format!(" on {interface}"));
                }
                output.push_str(&format!(" {family}"));
                match rule.protocol {
                    Protocol::Any => {}
                    Protocol::Tcp => output.push_str(" proto tcp"),
                    Protocol::Udp => output.push_str(" proto udp"),
                }
                // Remote peer is the source for inbound and destination for outbound.
                if *direction == "in" {
                    output.push_str(&format!(" from {endpoint} to any"));
                } else {
                    output.push_str(&format!(" from any to {endpoint}"));
                }
                if let Some(port) = rule.port {
                    output.push_str(&format!(" port {port}"));
                }
                if rule.action == Action::Allow {
                    output.push_str(" keep state");
                }
                output.push_str(&format!(" label \"rooklet_{}\"\n", rule.id));
            }
        }
    }
    Ok(output)
}

/// Render the PF preview without touching the system.
pub fn render_rules(rules: &[NetworkRule]) -> Result<String> {
    compile_rules(rules)
}
