//! CLI adapters for the shared profile service.
use super::args::ProfileCommand;
use crate::{
    auth::authenticate,
    json::{print_json, write_json},
};
use anyhow::{Context, Result, ensure};
use rooklet_core::{
    profile::export,
    text::{clean, clean_multiline},
};
use rooklet_macos::{backend::Backend, profile};
use std::fs;

pub(super) fn run(command: ProfileCommand) -> Result<()> {
    match command {
        ProfileCommand::Export { path } => {
            let mut backend = Backend::new()?;
            let profile = export(&backend.snapshot()?)?;
            if let Some(path) = path {
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .with_context(|| {
                        format!(
                            "cannot create {}; export never overwrites files",
                            path.display()
                        )
                    })?;
                write_json(&mut file, &profile)?;
                println!("Exported {}", clean(&path.display().to_string()));
            } else {
                print_json(&profile)?;
            }
        }
        ProfileCommand::Check { path } => {
            let profile = profile::read(&path)?;
            super::network_analysis::warn(&profile.network_rules)?;
            print_json(&profile)?;
        }
        ProfileCommand::Apply { path, yes } => {
            let profile = profile::read(&path)?;
            ensure!(
                yes,
                "review with `rooklet profile check PATH`, then pass --yes to replace all profile scopes"
            );
            authenticate()?;
            let mut backend = Backend::new()?;
            let prepared = profile::prepare(&profile, &backend.snapshot()?)?;
            eprintln!("{}", clean_multiline(&prepared.review()));
            profile::apply(&mut backend, &prepared)?;
            print_json(&backend.snapshot()?)?;
        }
    }
    Ok(())
}
