//! macOS incoming application firewall command adapter and readback verification.
use super::parser::{parse_applications, parse_settings};
use crate::{
    command,
    model::{Action, Application, FirewallSettings, Setting},
};
use anyhow::{Result, ensure};
use std::{path::Path, sync::atomic::AtomicBool};

const FIREWALL: &str = "/usr/libexec/ApplicationFirewall/socketfilterfw";

mod operations;
#[cfg(test)]
#[path = "alf/tests.rs"]
mod tests;

struct Runner<'a> {
    cancel: &'a AtomicBool,
}
impl operations::Runner for Runner<'_> {
    fn run(&mut self, program: &str, args: &[&str], privileged: bool) -> Result<String> {
        command::run(
            Path::new(program),
            &args.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
            None,
            privileged,
            self.cancel,
        )
    }
}

fn alf(args: &[&str], privileged: bool, cancel: &AtomicBool) -> Result<String> {
    command::run(
        Path::new(FIREWALL),
        &args.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        None,
        privileged,
        cancel,
    )
}
pub(super) fn read_settings(cancel: &AtomicBool) -> Result<FirewallSettings> {
    parse_settings(
        &alf(&["--getglobalstate"], false, cancel)?,
        &alf(&["--getstealthmode"], false, cancel)?,
        &alf(&["--getblockall"], false, cancel)?,
        &alf(&["--getallowsigned"], false, cancel)?,
    )
}
pub(super) fn read_applications(cancel: &AtomicBool) -> Result<Vec<Application>> {
    parse_applications(&alf(&["--listapps"], false, cancel)?)
}

pub(super) fn set_setting(setting: Setting, value: bool, cancel: &AtomicBool) -> Result<()> {
    let flag = match setting {
        Setting::Firewall => "--setglobalstate",
        Setting::Stealth => "--setstealthmode",
        Setting::BlockAll => "--setblockall",
        Setting::AllowSigned => "--setallowsigned",
        Setting::AllowSignedApp => "--setallowsignedapp",
    };
    alf(&[flag, if value { "on" } else { "off" }], true, cancel)?;
    ensure!(
        read_settings(cancel)?.get(setting) == value,
        "application firewall setting did not change; requested state could not be verified"
    );
    Ok(())
}

pub(super) fn set_applications(
    paths: &[String],
    action: Action,
    cancel: &AtomicBool,
) -> Result<()> {
    operations::set_applications(paths, action, &mut Runner { cancel })
}

pub(super) fn add_application(path: &str, cancel: &AtomicBool) -> Result<()> {
    operations::add_application(path, &mut Runner { cancel })
}

pub(super) fn remove_application(path: &str, cancel: &AtomicBool) -> Result<()> {
    operations::remove_application(path, &mut Runner { cancel })
}
