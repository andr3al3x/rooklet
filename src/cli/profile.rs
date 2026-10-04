//! Strict profile validation, export, and best-effort restoration.
use super::{applications::resolve_path, args::ProfileCommand, config::read_bounded};
use crate::{
    auth::authenticate,
    json::{print_json, write_json},
};
use anyhow::{Context, Result, bail, ensure};
use std::{fs, path::Path};
use xield::{
    backend::{Backend, registration_path, validate_application_path},
    model::{Action, Application, Mutation, Profile, Setting},
    network,
};

pub(super) fn run(backend: &mut Backend, command: ProfileCommand) -> Result<()> {
    match command {
        ProfileCommand::Export { path } => {
            let snapshot = backend.snapshot()?;
            ensure!(
                snapshot.firewall.is_some(),
                "cannot export unavailable firewall settings"
            );
            ensure!(
                snapshot.network.rules_available,
                "saved network rules are unavailable; run sudo -v before exporting"
            );
            ensure!(
                snapshot.applications_available,
                "cannot export unavailable incoming application entries"
            );
            let profile = Profile::from_snapshot(&snapshot);
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
                println!("Exported {}", path.display());
            } else {
                print_json(&profile)?;
            }
        }
        ProfileCommand::Check { path } => {
            let profile = read_profile(&path)?;
            print_json(&profile)?;
        }
        ProfileCommand::Apply { path, yes } => {
            let profile = read_profile(&path)?;
            ensure!(
                yes,
                "review with `xield profile check PATH`, then pass --yes to replace all profile scopes"
            );
            let current = backend.snapshot()?;
            ensure!(
                current.applications_available,
                "cannot replace unavailable incoming application entries"
            );
            let profile = normalize_applications(
                &profile,
                &current.applications,
                backend.is_demo(),
                &mut registration_path,
            )?;
            if !backend.is_demo() {
                for app in &profile.applications {
                    validate_application_path(&app.path, true)?;
                }
                authenticate()?;
            }
            apply_profile(backend, &profile)?;
            print_json(&backend.snapshot()?)?;
        }
    }
    Ok(())
}
fn read_profile(path: &Path) -> Result<Profile> {
    let profile: Profile = serde_json::from_slice(&read_bounded(fs::File::open(path)?)?)
        .context("invalid Xield profile")?;
    ensure!(
        profile.format == "xield-profile" && profile.version == 1,
        "unsupported profile format"
    );
    ensure!(
        profile.firewall.is_some(),
        "profile must contain firewall settings"
    );
    network::validate_rules(&profile.network_rules)?;
    ensure!(
        profile.applications.len() <= 4096,
        "too many application entries"
    );
    let mut paths = std::collections::HashSet::new();
    for app in &profile.applications {
        xield::backend::validate_application_path(&app.path, false)?;
        ensure!(paths.insert(&app.path), "duplicate application path");
    }
    Ok(profile)
}

fn normalize_applications(
    profile: &Profile,
    current: &[Application],
    demo: bool,
    resolve: &mut impl FnMut(&str) -> Result<String>,
) -> Result<Profile> {
    let mut profile = profile.clone();
    let mut paths = std::collections::HashSet::new();
    for app in &mut profile.applications {
        app.path = resolve_path(&app.path, current, demo, resolve)?;
        ensure!(
            paths.insert(app.path.clone()),
            "duplicate application registration after resolving paths: {:?}",
            app.path
        );
    }
    Ok(profile)
}

fn apply_permissions(backend: &mut Backend, profile: &Profile) -> Result<()> {
    let current = backend.snapshot()?;
    ensure!(
        current.applications_available,
        "incoming application entries are unavailable"
    );
    apply_permissions_with(&current.applications, profile, &mut |mutation| {
        backend.mutate(mutation)
    })
}

fn apply_permissions_with(
    old: &[Application],
    profile: &Profile,
    mutate: &mut impl FnMut(Mutation) -> Result<()>,
) -> Result<()> {
    if let Some(settings) = &profile.firewall {
        for setting in [
            Setting::Firewall,
            Setting::Stealth,
            Setting::BlockAll,
            Setting::AllowSigned,
            Setting::AllowSignedApp,
        ] {
            mutate(Mutation::Setting(setting, settings.get(setting)))?;
        }
    }
    for application in &profile.applications {
        if !old.iter().any(|app| app.path == application.path) {
            mutate(Mutation::AddApplication(application.path.clone()))?;
        }
        mutate(Mutation::Applications {
            paths: vec![application.path.clone()],
            action: if application.blocked {
                Action::Block
            } else {
                Action::Allow
            },
        })?;
    }
    for application in old {
        if !profile
            .applications
            .iter()
            .any(|app| app.path == application.path)
        {
            mutate(Mutation::RemoveApplication(application.path.clone()))?;
        }
    }
    Ok(())
}

fn apply_profile(backend: &mut Backend, profile: &Profile) -> Result<()> {
    let before = backend.snapshot()?;
    ensure!(
        before.firewall.is_some(),
        "cannot apply while firewall state is unavailable"
    );
    ensure!(
        before.network.rules_available,
        "saved network state is unavailable; authenticate before applying profiles"
    );
    ensure!(
        profile.network_rules.is_empty() || before.network.configured,
        "set up PF before applying a profile containing network rules"
    );
    ensure!(
        before.applications_available,
        "cannot replace unavailable incoming application entries"
    );
    let old = Profile::from_snapshot(&before);
    let result = (|| {
        apply_permissions(backend, profile)?;
        if before.network.configured {
            backend.mutate(Mutation::NetworkRules(profile.network_rules.clone()))?;
        }
        Ok::<_, anyhow::Error>(())
    })();
    if let Err(error) = result {
        let restored = apply_permissions(backend, &old);
        let network_restored = if before.network.configured {
            backend.mutate(Mutation::NetworkRules(old.network_rules))
        } else {
            Ok(())
        };
        bail!(
            "profile apply failed: {error}; restoration: incoming={}, network={}",
            restored
                .map(|_| "restored".to_string())
                .unwrap_or_else(|e| format!("FAILED ({e})")),
            network_restored
                .map(|_| "restored".to_string())
                .unwrap_or_else(|e| format!("FAILED ({e})"))
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, blocked: bool) -> Application {
        Application {
            path: path.into(),
            name: "App".into(),
            blocked,
        }
    }

    fn profile(applications: Vec<Application>) -> Profile {
        Profile {
            format: "xield-profile".into(),
            version: 1,
            firewall: None,
            applications,
            network_rules: Vec::new(),
        }
    }

    #[test]
    fn profile_resolves_new_bundles_and_retains_existing_raw_registrations() {
        let current = [entry("/registered/Alias.app", false)];
        let input = profile(vec![
            entry("/registered/Alias.app", true),
            entry("/Applications/New.app", false),
        ]);
        let output = normalize_applications(&input, &current, false, &mut |path| {
            assert_eq!(path, "/Applications/New.app");
            Ok("/canonical/NewExecutable".into())
        })
        .unwrap();
        assert_eq!(output.applications[0].path, "/registered/Alias.app");
        assert!(output.applications[0].blocked);
        assert_eq!(output.applications[1].path, "/canonical/NewExecutable");
        assert_eq!(input.applications[1].path, "/Applications/New.app");
    }

    #[test]
    fn profile_rejects_aliases_for_an_already_requested_registration() {
        let input = profile(vec![
            entry("/canonical/App", true),
            entry("/Applications/App.app", false),
        ]);
        let current = [entry("/canonical/App", false)];
        let error =
            normalize_applications(
                &input,
                &current,
                false,
                &mut |_| Ok("/canonical/App".into()),
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("duplicate application registration")
        );

        let input = profile(vec![
            entry("/Applications/App.app", true),
            entry("/symlink/App", false),
        ]);
        assert!(
            normalize_applications(&input, &[], false, &mut |_| { Ok("/canonical/App".into()) })
                .is_err()
        );
    }

    #[test]
    fn profile_updates_existing_raw_registration_without_readding_it() {
        let old = [
            entry("/registered/Alias.app", false),
            entry("/registered/RemovedHelper", false),
        ];
        let input = profile(vec![
            entry("/registered/Alias.app", true),
            entry("/canonical/NewExecutable", false),
        ]);
        let mut mutations = Vec::new();
        apply_permissions_with(&old, &input, &mut |mutation| {
            mutations.push(mutation);
            Ok(())
        })
        .unwrap();
        assert_eq!(mutations.len(), 4);
        assert!(matches!(
            &mutations[0],
            Mutation::Applications { paths, action: Action::Block }
                if paths == &["/registered/Alias.app"]
        ));
        assert!(matches!(
            &mutations[1],
            Mutation::AddApplication(path) if path == "/canonical/NewExecutable"
        ));
        assert!(matches!(
            &mutations[2],
            Mutation::Applications { paths, action: Action::Allow }
                if paths == &["/canonical/NewExecutable"]
        ));
        assert!(matches!(
            &mutations[3],
            Mutation::RemoveApplication(path) if path == "/registered/RemovedHelper"
        ));
    }

    #[test]
    fn demo_profile_keeps_nonexistent_paths_and_applies_exact_permissions() {
        let mut backend = Backend::new(true).unwrap();
        let before = backend.snapshot().unwrap();
        let mut input = Profile::from_snapshot(&before);
        input.applications = vec![entry("/nonexistent/App.app", true)];
        let normalized = normalize_applications(&input, &before.applications, true, &mut |_| {
            panic!("demo must not query files")
        })
        .unwrap();
        apply_profile(&mut backend, &normalized).unwrap();
        assert_eq!(backend.snapshot().unwrap().applications, input.applications);
    }

    #[test]
    fn profile_check_accepts_normalized_paths_without_requiring_files() {
        let mut backend = Backend::new(true).unwrap();
        let mut input = Profile::from_snapshot(&backend.snapshot().unwrap());
        input.applications = vec![entry("/nonexistent/App.app", true)];
        let mut file = tempfile::NamedTempFile::new().unwrap();
        write_json(&mut file, &input).unwrap();
        assert_eq!(
            read_profile(file.path()).unwrap().applications,
            input.applications
        );
    }
}
