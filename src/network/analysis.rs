//! Pure explanations of Xield's ordered quick rules, independent of live PF state.
use super::{
    compiler::{destination, valid_interface},
    validate_rules,
};
use crate::model::{Action, Direction, NetworkRule, Protocol};
use anyhow::{Result, ensure};
use ipnet::IpNet;
use serde::Serialize;
use std::net::IpAddr;

/// A hypothetical packet. Inbound destination ports identify the local service;
/// outbound destination ports identify the remote service. Unknown fields remain
/// unknown: observed remote ports must not be substituted for inbound local ports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleQuery {
    pub remote_ip: IpAddr,
    pub protocol: Protocol,
    pub direction: Direction,
    pub destination_port: Option<u16>,
    pub interface: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryField {
    DestinationPort,
    Interface,
}

/// Positions refer to the original ordered rule list and are one-based.
/// This explains only Xield rule matching, never effective machine enforcement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum RuleExplanation {
    Matched {
        rule_id: String,
        position: usize,
        action: Action,
    },
    Indeterminate {
        rule_id: String,
        position: usize,
        missing_fields: Vec<QueryField>,
    },
    NoMatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShadowWarning {
    pub shadowed_id: String,
    pub shadowed_position: usize,
    pub covering_id: String,
    pub covering_position: usize,
}

/// Find the first possible enabled match. A possible earlier match with unknown
/// fields prevents claiming that a later definite match wins.
pub fn explain_rules(rules: &[NetworkRule], query: &RuleQuery) -> Result<RuleExplanation> {
    validate_rules(rules)?;
    ensure!(
        query.protocol != Protocol::Any,
        "Query protocol must be TCP or UDP"
    );
    ensure!(
        query.direction != Direction::Both,
        "Query direction must be inbound or outbound"
    );
    ensure!(
        query.destination_port != Some(0),
        "Query destination port must be between 1 and 65535"
    );
    if let Some(interface) = &query.interface {
        ensure!(valid_interface(interface), "Query interface is invalid");
    }
    for (index, rule) in rules.iter().enumerate().filter(|(_, rule)| rule.enabled) {
        if (rule.protocol != Protocol::Any && rule.protocol != query.protocol)
            || (rule.direction != Direction::Both && rule.direction != query.direction)
            || destination(&rule.destination)?.is_some_and(|net| !net.contains(&query.remote_ip))
            || matches!((rule.port, query.destination_port), (Some(a), Some(b)) if a != b)
            || matches!((&rule.interface, &query.interface), (Some(a), Some(b)) if a != b)
        {
            continue;
        }
        let mut missing_fields = Vec::new();
        if rule.port.is_some() && query.destination_port.is_none() {
            missing_fields.push(QueryField::DestinationPort);
        }
        if rule.interface.is_some() && query.interface.is_none() {
            missing_fields.push(QueryField::Interface);
        }
        return Ok(if missing_fields.is_empty() {
            RuleExplanation::Matched {
                rule_id: rule.id.clone(),
                position: index + 1,
                action: rule.action,
            }
        } else {
            RuleExplanation::Indeterminate {
                rule_id: rule.id.clone(),
                position: index + 1,
                missing_fields,
            }
        });
    }
    Ok(RuleExplanation::NoMatch)
}

fn covers(
    earlier: &NetworkRule,
    earlier_net: Option<IpNet>,
    later: &NetworkRule,
    later_net: Option<IpNet>,
) -> bool {
    let address_covers = match (earlier_net, later_net) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(a), Some(b)) => a.prefix_len() <= b.prefix_len() && a.contains(&b.network()),
    };
    address_covers
        && (earlier.protocol == Protocol::Any || earlier.protocol == later.protocol)
        && (earlier.direction == Direction::Both || earlier.direction == later.direction)
        && (earlier.port.is_none() || earlier.port == later.port)
        && (earlier.interface.is_none() || earlier.interface == later.interface)
}

/// Warn only when a single earlier enabled rule completely covers a later one.
/// Coverage by a union of earlier rules is deliberately not inferred. Actions
/// do not affect reachability: either action is quick and stops evaluation.
pub fn shadow_warnings(rules: &[NetworkRule]) -> Result<Vec<ShadowWarning>> {
    validate_rules(rules)?;
    let networks = rules
        .iter()
        .map(|rule| destination(&rule.destination))
        .collect::<Result<Vec<_>>>()?;
    let mut warnings = Vec::new();
    for (index, later) in rules.iter().enumerate().filter(|(_, rule)| rule.enabled) {
        if let Some((cover_index, earlier)) =
            rules[..index]
                .iter()
                .enumerate()
                .find(|(earlier_index, earlier)| {
                    earlier.enabled
                        && covers(earlier, networks[*earlier_index], later, networks[index])
                })
        {
            warnings.push(ShadowWarning {
                shadowed_id: later.id.clone(),
                shadowed_position: index + 1,
                covering_id: earlier.id.clone(),
                covering_position: cover_index + 1,
            });
        }
    }
    Ok(warnings)
}
