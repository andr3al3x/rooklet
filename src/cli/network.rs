//! PF CLI operations and ruleset editing.
use super::{args::NetworkCommand, config::input_rules};
use crate::{auth::authenticate, json::print_json};
use anyhow::{Context, Result, ensure};
use std::{sync::atomic::AtomicBool, time::Duration};
use xield::{
    backend::Backend,
    model::{Mutation, NetworkRule, NetworkStatus},
    network,
};

fn network_status(demo: bool) -> Result<NetworkStatus> {
    if demo {
        return Ok(Backend::new(true)?.snapshot()?.network);
    }
    if xield::command::is_root() {
        return Ok(network::status());
    }
    let result = xield::command::run(
        &std::env::current_exe()?,
        &["network".into(), "status".into()],
        None,
        true,
        &AtomicBool::new(false),
    );
    match result {
        Ok(text) => Ok(serde_json::from_str(&text)?),
        Err(_) => Ok(NetworkStatus {
            message: Some(
                "PF status requires administrator access; run sudo -v or sudo xield network status"
                    .into(),
            ),
            ..Default::default()
        }),
    }
}
enum NetworkChange<'a> {
    Setup(&'a [NetworkRule]),
    Apply(&'a [NetworkRule]),
    Disable,
    Remove,
}

fn network_change(change: NetworkChange<'_>) -> Result<()> {
    if xield::command::is_root() {
        return match change {
            NetworkChange::Setup(rules) => network::setup(rules),
            NetworkChange::Apply(rules) => network::apply(rules),
            NetworkChange::Disable => network::disable(),
            NetworkChange::Remove => network::remove(),
        };
    }
    let (action, rules) = match change {
        NetworkChange::Setup(rules) => ("setup", Some(rules)),
        NetworkChange::Apply(rules) => ("apply", Some(rules)),
        NetworkChange::Disable => ("disable", None),
        NetworkChange::Remove => ("remove", None),
    };
    authenticate()?;
    let mut args = vec!["network".into(), action.into()];
    let input = rules.map(serde_json::to_vec).transpose()?;
    if input.is_some() {
        args.push("--stdin".into());
    }
    xield::command::run_with_timeout(
        &std::env::current_exe()?,
        &args,
        input.as_deref(),
        true,
        &AtomicBool::new(false),
        Duration::from_secs(90),
    )?;
    Ok(())
}
pub(super) fn run(command: NetworkCommand, demo: bool) -> Result<()> {
    let setup = matches!(&command, NetworkCommand::Setup(_));
    match command {
        NetworkCommand::Status => return print_json(&network_status(demo)?),
        NetworkCommand::Preview(input) => {
            print!("{}", network::render_rules(&input_rules(&input, false)?)?);
            return Ok(());
        }
        NetworkCommand::Setup(input) | NetworkCommand::Apply(input) => {
            let rules = input_rules(&input, setup)?;
            if demo {
                let mut backend = Backend::new(true)?;
                backend.mutate(Mutation::NetworkRules(rules))?;
                return print_json(&backend.snapshot()?.network);
            }
            network_change(if setup {
                NetworkChange::Setup(&rules)
            } else {
                NetworkChange::Apply(&rules)
            })?;
        }
        NetworkCommand::Disable | NetworkCommand::Remove => {
            let remove = matches!(command, NetworkCommand::Remove);
            if demo {
                return print_json(&NetworkStatus {
                    message: Some("Demo only; no PF settings changed".into()),
                    ..Default::default()
                });
            }
            network_change(if remove {
                NetworkChange::Remove
            } else {
                NetworkChange::Disable
            })?;
        }
        command => {
            if !demo {
                authenticate()?;
            }
            let status = network_status(demo)?;
            ensure!(
                status.rules_available,
                "saved PF rules are unavailable; authenticate before changing them"
            );
            ensure!(
                status.configured,
                "set up PF first: sudo xield network setup"
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
            if demo {
                let mut backend = Backend::new(true)?;
                backend.mutate(Mutation::NetworkRules(rules))?;
                return print_json(&backend.snapshot()?.network);
            }
            network_change(NetworkChange::Apply(&rules))?;
        }
    }
    print_json(&network_status(false)?)
}
