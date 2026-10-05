//! Bounded JSON configuration input.
use super::args::InputRules;
use anyhow::{Context, Result, ensure};
use rooklet::{model::NetworkRule, network};
use std::{
    fs,
    io::{self, Read},
};

pub(super) fn read_bounded(mut reader: impl Read) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    reader
        .by_ref()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut data)?;
    ensure!(data.len() <= 1024 * 1024, "configuration exceeds 1 MiB");
    Ok(data)
}
pub(super) fn input_rules(input: &InputRules, empty_ok: bool) -> Result<Vec<NetworkRule>> {
    let data = if input.stdin {
        read_bounded(io::stdin().lock())?
    } else if let Some(path) = &input.path {
        read_bounded(fs::File::open(path)?)?
    } else {
        ensure!(empty_ok, "supply a JSON rule file or --stdin");
        return Ok(Vec::new());
    };
    let rules = serde_json::from_slice::<Vec<NetworkRule>>(&data)
        .context("expected a JSON array of network rules")?;
    network::validate_rules(&rules)?;
    Ok(rules)
}
