//! Fixed-path PF commands, ownership references, and interface checks.
use super::config::{normalized_rules, validate_parent_rules};
use crate::command;
use anyhow::{Context, Result, ensure};
use rooklet_core::model::NetworkRule;
use std::{path::Path, sync::atomic::AtomicBool};

const PFCTL: &str = "/sbin/pfctl";

pub(super) fn run(args: &[&str], input: Option<&[u8]>) -> Result<String> {
    command::run(
        Path::new(PFCTL),
        &args.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        input,
        // SAFETY: geteuid takes no arguments and has no caller-side preconditions.
        unsafe { libc::geteuid() } != 0,
        &AtomicBool::new(false),
    )
}
pub(super) fn acquire_token() -> Result<String> {
    let output = command::run_combined(
        Path::new(PFCTL),
        &["-E".into()],
        None,
        false,
        &AtomicBool::new(false),
    )?;
    enable_token(&output)
}
pub(super) fn root() -> Result<()> {
    ensure!(cfg!(target_os = "macos"), "PF management requires macOS");
    ensure!(
        // SAFETY: geteuid takes no arguments and has no caller-side preconditions.
        unsafe { libc::geteuid() } == 0,
        "Network changes require root; run this command through sudo"
    );
    Ok(())
}
pub(super) fn check_parent_rules() -> Result<()> {
    let filters = run(&["-sr"], None)?;
    let translations = run(&["-sn"], None)?;
    validate_parent_rules(&filters, &translations)
}

pub(super) fn valid_token(token: &str) -> bool {
    !token.is_empty() && token.len() <= 32 && token.bytes().all(|c| c.is_ascii_digit())
}
fn enable_token(output: &str) -> Result<String> {
    let token = output
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("Token :")
                .or_else(|| line.trim().strip_prefix("Token:"))
        })
        .map(str::trim)
        .context(
            "pfctl enabled PF but did not return an ownership token; inspect PF before retrying",
        )?;
    ensure!(valid_token(token), "pfctl returned an invalid enable token");
    Ok(token.into())
}
pub(super) fn token_is_live(token: &str) -> Result<bool> {
    // Tokens may be stale after reboot. Never treat a saved token as a live PF reference.
    let references = run(&["-s", "References"], None)?;
    Ok(references
        .split(|c: char| !c.is_ascii_digit())
        .any(|value| value == token))
}
pub(super) fn validate_pf(source: &str) -> Result<String> {
    run(
        &["-n", "-v", "-a", "rooklet", "-f", "-"],
        Some(source.as_bytes()),
    )
    .map(|output| normalized_rules(&output))
}
pub(super) fn load_anchor(source: &str) -> Result<()> {
    run(&["-a", "rooklet", "-f", "-"], Some(source.as_bytes())).map(|_| ())
}
pub(super) fn verify_interfaces(rules: &[NetworkRule]) -> Result<()> {
    for rule in rules.iter().filter(|r| r.enabled) {
        if let Some(interface) = &rule.interface {
            let name = std::ffi::CString::new(interface.as_str())?;
            ensure!(
                // SAFETY: name is NUL-terminated and remains live for this read-only call.
                unsafe { libc::if_nametoindex(name.as_ptr()) } != 0,
                "Interface {interface} does not exist"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ownership_tokens_are_strictly_numeric() {
        assert_eq!(
            enable_token("pf enabled\nToken : 12345\n").unwrap(),
            "12345"
        );
        assert!(enable_token("Token : 123; pfctl -d").is_err());
        assert!(enable_token("pf enabled").is_err());
    }
}
