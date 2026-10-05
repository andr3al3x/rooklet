//! PF CLI operations and ruleset editing.
use super::{args::NetworkCommand, config::input_rules};
use crate::{auth::authenticate, json::print_json};
use anyhow::{Context, Result, ensure};
use rooklet_core::{
    model::{NetworkRule, NetworkStatus},
    network,
};
use rooklet_macos::network::{Change, request_change, request_preflight, request_status};
use std::sync::atomic::AtomicBool;

pub(super) fn network_status() -> Result<NetworkStatus> {
    request_status(&AtomicBool::new(false))
}

fn network_change(change: Change<'_>) -> Result<()> {
    authenticate()?;
    request_change(change, &AtomicBool::new(false))
}

pub(super) fn run(command: NetworkCommand) -> Result<()> {
    let setup = matches!(&command, NetworkCommand::Setup(_));
    match command {
        NetworkCommand::Status => return print_json(&network_status()?),
        NetworkCommand::Check(input) => return super::network_analysis::check(&input),
        NetworkCommand::Explain {
            input,
            remote,
            protocol,
            direction,
            port,
            interface,
        } => {
            return super::network_analysis::explain(
                &input,
                network::RuleQuery {
                    remote_ip: remote,
                    protocol,
                    direction,
                    destination_port: port,
                    interface,
                },
            );
        }
        NetworkCommand::Preflight(input) => {
            let rules = input_rules(&input, false)?;
            authenticate()?;
            request_preflight(&rules, &AtomicBool::new(false))?;
            println!("PF preflight passed; firewall state is unchanged.");
            return Ok(());
        }
        NetworkCommand::Preview(input) => {
            print!("{}", network::render_rules(&input_rules(&input, false)?)?);
            return Ok(());
        }
        NetworkCommand::Setup(input) | NetworkCommand::Apply(input) => {
            let rules = input_rules(&input, setup)?;
            super::network_analysis::warn(&rules)?;
            network_change(if setup {
                Change::Setup(&rules)
            } else {
                Change::Apply(&rules)
            })?;
        }
        NetworkCommand::Disable | NetworkCommand::Remove => {
            let remove = matches!(command, NetworkCommand::Remove);
            network_change(if remove {
                Change::Remove
            } else {
                Change::Disable
            })?;
        }
        command => {
            authenticate()?;
            let status = network_status()?;
            ensure!(
                status.rules_available,
                "saved PF rules are unavailable; authenticate before changing them"
            );
            ensure!(
                status.configured,
                "set up PF first: sudo rooklet network setup"
            );
            let mut rules = status.rules;
            match command {
                NetworkCommand::Add(args) => {
                    let rule = NetworkRule {
                        id: args.id.unwrap_or_else(|| {
                            format!(
                                "rule-{}",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_nanos()
                            )
                        }),
                        name: args.name,
                        action: args.action,
                        destination: args.destination,
                        port: args.port,
                        protocol: args.protocol,
                        direction: args.direction,
                        interface: args.interface,
                        enabled: true,
                    };
                    ensure!(
                        !rules.iter().any(|r| r.id == rule.id),
                        "rule ID already exists; use network apply to replace a ruleset"
                    );
                    rules.push(rule);
                }
                NetworkCommand::Delete { id } => {
                    let index = rules
                        .iter()
                        .position(|r| r.id == id)
                        .context("rule not found")?;
                    rules.remove(index);
                }
                NetworkCommand::Toggle { id } => {
                    let rule = rules
                        .iter_mut()
                        .find(|r| r.id == id)
                        .context("rule not found")?;
                    rule.enabled = !rule.enabled;
                }
                NetworkCommand::Move { id, offset } => {
                    ensure!(offset != 0, "offset must be -1 or 1");
                    let index = rules
                        .iter()
                        .position(|r| r.id == id)
                        .context("rule not found")?;
                    let target = index
                        .checked_add_signed(offset as isize)
                        .filter(|i| *i < rules.len())
                        .context("rule is already at the boundary")?;
                    rules.swap(index, target);
                }
                _ => unreachable!(),
            }
            network::validate_rules(&rules)?;
            super::network_analysis::warn(&rules)?;
            network_change(Change::Apply(&rules))?;
        }
    }
    print_json(&network_status()?)
}
