//! Strict socketfilterfw output parsing.
use super::application::{application_name, validate_application_path};
use crate::model::{Application, FirewallSettings};
use anyhow::{Context, Result, bail, ensure};
use std::collections::HashSet;

pub fn parse_settings(
    global: &str,
    stealth: &str,
    block_all: &str,
    signed: &str,
) -> Result<FirewallSettings> {
    let enabled = match global.trim() {
        "Firewall is disabled. (State = 0)" => false,
        "Firewall is enabled. (State = 1)"
        | "Firewall is enabled. (State = 2)"
        | "Firewall is set to block all non-essential incoming connections" => true,
        _ => bail!("unrecognized application firewall global state"),
    };
    let stealth = match stealth.trim() {
        "Firewall stealth mode is on" => true,
        "Firewall stealth mode is off" => false,
        _ => bail!("unrecognized application firewall stealth state"),
    };
    let block_all = match block_all.trim() {
        "Firewall has block all state set to enabled." => true,
        "Firewall has block all state set to disabled." => false,
        _ => bail!("unrecognized application firewall block-all state"),
    };
    let (mut built_in, mut downloaded) = (None, None);
    for line in signed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let (target, state) = if let Some(state) =
            line.strip_prefix("Automatically allow built-in signed software ")
        {
            (&mut built_in, state)
        } else if let Some(state) =
            line.strip_prefix("Automatically allow downloaded signed software ")
        {
            (&mut downloaded, state)
        } else {
            bail!("unrecognized application firewall signed software state");
        };
        ensure!(target.is_none(), "duplicate signed software state");
        *target = Some(match state {
            "ENABLED." => true,
            "DISABLED." => false,
            _ => bail!("unrecognized signed software value"),
        });
    }
    Ok(FirewallSettings {
        enabled,
        stealth,
        block_all,
        allow_signed: built_in.context("missing built-in signed software state")?,
        allow_signed_app: downloaded.context("missing downloaded signed software state")?,
    })
}
pub fn parse_applications(output: &str) -> Result<Vec<Application>> {
    let mut lines = output.lines().filter(|line| !line.trim().is_empty());
    let count: usize = lines
        .next()
        .context("missing application firewall app count")?
        .trim()
        .strip_prefix("Total number of apps = ")
        .context("unrecognized application firewall app count")?
        .trim()
        .parse()
        .context("invalid application firewall app count")?;
    ensure!(
        count <= 10000,
        "application firewall list exceeds 10000 applications"
    );
    let mut apps = Vec::new();
    let mut paths = HashSet::new();
    while let Some(line) = lines.next() {
        let (index, path) = line
            .trim_start()
            .split_once(" : ")
            .context("unrecognized application firewall app entry")?;
        // ALF appends one formatting space after the path. Preserve any spaces
        // belonging to the filename instead of trimming the entire field.
        let path = path.strip_suffix(' ').unwrap_or(path);
        ensure!(
            index.parse::<usize>().ok() == Some(apps.len() + 1),
            "invalid application firewall app index"
        );
        validate_application_path(path, false)?;
        ensure!(
            paths.insert(path.to_owned()),
            "duplicate application firewall app path"
        );
        let blocked = match lines
            .next()
            .context("application firewall app lacks connection state")?
            .trim()
        {
            "( Allow incoming connections )" | "(Allow incoming connections)" => false,
            "( Block incoming connections )" | "(Block incoming connections)" => true,
            _ => bail!("unrecognized application firewall app connection state"),
        };
        apps.push(Application {
            path: path.into(),
            name: application_name(path),
            blocked,
        });
    }
    ensure!(
        apps.len() == count,
        "application firewall app count mismatch"
    );
    Ok(apps)
}
pub fn parse_app_blocked(output: &str, path: &str) -> Result<bool> {
    let suffix = output
        .trim()
        .strip_prefix(&format!("Incoming connection to {path} is "))
        .context("unrecognized application firewall application state")?;
    match suffix {
        "blocked." | "blocked" => Ok(true),
        "permitted." | "permitted" => Ok(false),
        _ => bail!("unrecognized application firewall application state"),
    }
}
