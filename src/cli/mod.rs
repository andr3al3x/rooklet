//! CLI orchestration. Privileged operations are delegated to the backend.
mod applications;
mod args;
mod config;
mod geoip;
mod network;
mod network_analysis;
mod profile;

use crate::{auth::authenticate, json::print_json};
use anyhow::{Result, ensure};
use args::{AppsCommand, Cli, CliCommand, FirewallCommand, Switch};
use clap::Parser;
use rooklet_core::{
    model::{Action, Mutation},
    text::{clean, clean_multiline},
};
use rooklet_macos::backend::Backend;
use std::io::{self, IsTerminal};

pub(crate) fn run() -> Result<()> {
    let cli = Cli::try_parse().unwrap_or_else(|error| {
        let diagnostic = clean_multiline(&error.to_string());
        if error.use_stderr() {
            eprint!("{diagnostic}");
        } else {
            print!("{diagnostic}");
        }
        std::process::exit(error.exit_code());
    });
    if let Some(command) = cli.command {
        if let CliCommand::Network { command } = command {
            return network::run(command);
        }
        if let CliCommand::Geoip { command } = command {
            return geoip::run(command);
        }
        if let CliCommand::Profile { command } = command {
            return profile::run(command);
        }
        let mut backend = Backend::new()?;
        return run_command(&mut backend, command);
    }
    ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "the TUI needs an interactive terminal; use `rooklet status` for JSON"
    );
    crate::tui::run(cli.theme)
}
fn run_command(backend: &mut Backend, command: CliCommand) -> Result<()> {
    match command {
        CliCommand::Status => print_json(&backend.snapshot()?)?,
        CliCommand::Doctor => {
            let snapshot = backend.snapshot()?;
            println!("Rooklet uses the macOS application firewall and PF.");
            println!(
                "Incoming firewall: {}",
                snapshot
                    .firewall
                    .as_ref()
                    .map(|s| if s.enabled { "on" } else { "off" })
                    .unwrap_or("unavailable")
            );
            println!(
                "PF: {}",
                clean(snapshot.network.message.as_deref().unwrap_or(
                    if snapshot.network.configured {
                        "configured"
                    } else {
                        "optional; not configured"
                    }
                ))
            );
            println!(
                "Country database: {}",
                clean(snapshot.geoip.as_deref().unwrap_or("optional; not loaded"))
            );
            for notice in snapshot.notices {
                println!("{}", clean_multiline(&notice));
            }
            ensure!(
                snapshot.firewall.is_some(),
                "application firewall could not be read"
            );
        }
        CliCommand::Apps { command } => match command {
            AppsCommand::List => {
                let snapshot = backend.snapshot()?;
                ensure!(
                    snapshot.applications_available,
                    "incoming application entries are unavailable; see rooklet status diagnostics"
                );
                print_json(&snapshot.applications)?;
            }
            AppsCommand::Add { path } => mutate_cli(backend, Mutation::AddApplication(path))?,
            AppsCommand::Allow { path } => mutate_cli(
                backend,
                Mutation::Applications {
                    paths: vec![path],
                    action: Action::Allow,
                },
            )?,
            AppsCommand::Block { path } => mutate_cli(
                backend,
                Mutation::Applications {
                    paths: vec![path],
                    action: Action::Block,
                },
            )?,
            AppsCommand::Remove { path } => mutate_cli(backend, Mutation::RemoveApplication(path))?,
        },
        CliCommand::Firewall { command } => match command {
            FirewallCommand::Status => print_json(&backend.snapshot()?.firewall)?,
            FirewallCommand::Set { setting, state } => mutate_cli(
                backend,
                Mutation::Setting(setting, matches!(state, Switch::On)),
            )?,
        },
        CliCommand::Profile { command } => profile::run(command)?,
        CliCommand::Network { command } => network::run(command)?,
        CliCommand::Geoip { command } => geoip::run(command)?,
    }
    Ok(())
}
fn mutate_cli(backend: &mut Backend, mutation: Mutation) -> Result<()> {
    let mutation = applications::preflight(backend, mutation)?;
    authenticate()?;
    backend.mutate(mutation)?;
    print_json(&backend.snapshot()?)
}
