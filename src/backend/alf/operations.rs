//! Exact registered-target ALF operations, with readback and explicit partial failures.
use super::FIREWALL;
use crate::{
    backend::{
        application::{
            registration_path_with, validate_application_path, validate_application_targets,
        },
        parser::{parse_app_blocked, parse_applications},
    },
    model::{Action, Application},
};
use anyhow::{Context, Result, bail, ensure};

pub(super) trait Runner {
    fn run(&mut self, program: &str, args: &[&str], privileged: bool) -> Result<String>;
}

fn applications(runner: &mut impl Runner) -> Result<Vec<Application>> {
    parse_applications(&runner.run(FIREWALL, &["--listapps"], false)?)
}

pub(super) fn set_applications(
    paths: &[String],
    action: Action,
    runner: &mut impl Runner,
) -> Result<()> {
    validate_application_targets(paths, true)?;
    let initial = applications(runner)?;
    for path in paths {
        ensure!(
            initial.iter().any(|app| &app.path == path),
            "application target {path:?} is not registered; add it first"
        );
    }
    let blocked = action == Action::Block;
    let mut changed = Vec::new();
    let flag = if blocked {
        "--blockapp"
    } else {
        "--unblockapp"
    };
    for (index, path) in paths.iter().enumerate() {
        let result = (|| {
            runner.run(FIREWALL, &[flag, path], true)?;
            let output = runner.run(FIREWALL, &["--getappblocked", path], false)?;
            ensure!(
                parse_app_blocked(&output, path)? == blocked,
                "requested incoming permission could not be verified"
            );
            ensure!(
                applications(runner)?
                    .iter()
                    .any(|app| &app.path == path && app.blocked == blocked),
                "application firewall registration could not be verified"
            );
            Ok::<_, anyhow::Error>(())
        })();
        if let Err(error) = result {
            bail!(
                "incoming permission action failed for {path:?}: {error:#}; previously verified changed entries: {changed:?}; this target may have changed; {} remaining entries were not attempted; ALF changes are not atomic",
                paths.len() - index - 1
            );
        }
        if initial
            .iter()
            .any(|app| &app.path == path && app.blocked != blocked)
        {
            changed.push(path);
        }
    }
    Ok(())
}

pub(super) fn add_application(path: &str, runner: &mut impl Runner) -> Result<()> {
    let registered = registration_path_with(path, &mut |plist| {
        runner.run(
            "/usr/bin/plutil",
            &[
                "-extract",
                "CFBundleExecutable",
                "raw",
                "-expect",
                "string",
                "-o",
                "-",
                plist,
            ],
            false,
        )
    })?;
    runner.run(FIREWALL, &["--add", &registered], true)?;
    ensure!(
        applications(runner)
            .context("application add command completed, but readback failed; registration may have changed")?
            .iter()
            .any(|app| app.path == registered),
        "application add could not be verified; expected executable registration {registered:?} is absent"
    );
    Ok(())
}

pub(super) fn remove_application(path: &str, runner: &mut impl Runner) -> Result<()> {
    validate_application_path(path, false)?;
    ensure!(
        applications(runner)?.iter().any(|app| app.path == path),
        "application is not registered at the captured path"
    );
    runner.run(FIREWALL, &["--remove", path], true)?;
    ensure!(
        !applications(runner)
            .context("application remove command completed, but readback failed; registration may have changed")?
            .iter()
            .any(|app| app.path == path),
        "application removal could not be verified; captured registration is still present"
    );
    Ok(())
}
