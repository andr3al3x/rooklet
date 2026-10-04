//! Resolve CLI input to exact ALF registrations before authentication.
use anyhow::{Result, ensure};
use xield::{
    backend::{Backend, registration_path, validate_application_path},
    model::{Application, Mutation},
};

pub(super) fn resolve_path(
    path: &str,
    current: &[Application],
    demo: bool,
    resolve: &mut impl FnMut(&str) -> Result<String>,
) -> Result<String> {
    validate_application_path(path, false)?;
    if demo || current.iter().any(|app| app.path == path) {
        return Ok(path.to_owned());
    }
    resolve(path)
}

fn registered_path(
    path: &str,
    current: &[Application],
    demo: bool,
    resolve: &mut impl FnMut(&str) -> Result<String>,
) -> Result<String> {
    let path = resolve_path(path, current, demo, resolve)?;
    ensure!(
        current.iter().any(|app| app.path == path),
        "application is not registered at {path:?}; add it first"
    );
    Ok(path)
}

pub(super) fn preflight(backend: &mut Backend, mutation: Mutation) -> Result<Mutation> {
    match mutation {
        Mutation::AddApplication(path) => {
            validate_application_path(&path, false)?;
            let path = if backend.is_demo() {
                path
            } else {
                registration_path(&path)?
            };
            Ok(Mutation::AddApplication(path))
        }
        Mutation::Applications { paths, action } => {
            let current = backend.snapshot()?;
            ensure!(
                current.applications_available,
                "incoming application entries are unavailable"
            );
            let paths = paths
                .iter()
                .map(|path| {
                    let path = registered_path(
                        path,
                        &current.applications,
                        backend.is_demo(),
                        &mut registration_path,
                    )?;
                    validate_application_path(&path, !backend.is_demo())?;
                    Ok(path)
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Mutation::Applications { paths, action })
        }
        Mutation::RemoveApplication(path) => {
            let current = backend.snapshot()?;
            ensure!(
                current.applications_available,
                "incoming application entries are unavailable"
            );
            Ok(Mutation::RemoveApplication(registered_path(
                &path,
                &current.applications,
                backend.is_demo(),
                &mut registration_path,
            )?))
        }
        mutation => Ok(mutation),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str) -> Application {
        Application {
            path: path.into(),
            name: "App".into(),
            blocked: false,
        }
    }

    #[test]
    fn exact_registered_alias_is_retained_without_resolution() {
        let current = [entry("/Applications/App.app"), entry("/real/App")];
        let path = registered_path("/Applications/App.app", &current, false, &mut |_| {
            panic!("exact registration must not resolve")
        })
        .unwrap();
        assert_eq!(path, "/Applications/App.app");
    }

    #[test]
    fn bundle_input_selects_only_its_registered_executable() {
        let current = [entry("/real/App"), entry("/real/Helper")];
        let path = registered_path("/Applications/App.app", &current, false, &mut |path| {
            assert_eq!(path, "/Applications/App.app");
            Ok("/real/App".into())
        })
        .unwrap();
        assert_eq!(path, "/real/App");
        assert!(
            registered_path("/Applications/Other.app", &current, false, &mut |_| {
                Ok("/real/Other".into())
            })
            .is_err()
        );
    }

    #[test]
    fn demo_paths_do_not_resolve_or_require_files() {
        let current = [entry("/nonexistent/App.app")];
        assert!(
            registered_path("/nonexistent/App.app", &current, true, &mut |_| {
                panic!("demo must not resolve files")
            })
            .is_ok()
        );
        assert!(
            registered_path("/nonexistent/Other.app", &current, true, &mut |_| {
                panic!("demo must not resolve files")
            })
            .is_err()
        );
    }

    #[test]
    fn demo_preflight_requires_registration_before_permission_or_removal() {
        let mut backend = Backend::new(true).unwrap();
        for mutation in [
            Mutation::Applications {
                paths: vec!["/nonexistent/Unknown.app".into()],
                action: xield::model::Action::Block,
            },
            Mutation::RemoveApplication("/nonexistent/Unknown.app".into()),
        ] {
            assert!(preflight(&mut backend, mutation).is_err());
        }
        assert!(
            preflight(
                &mut backend,
                Mutation::AddApplication("/nonexistent/New.app".into())
            )
            .is_ok()
        );
    }

    #[test]
    fn cli_actions_affect_only_the_requested_registration() {
        let mut backend = Backend::new(true).unwrap();
        let path = "/nonexistent/App.app/Contents/MacOS/Main";
        let helper = "/nonexistent/App.app/Contents/MacOS/Helper";
        for path in [path, helper] {
            backend
                .mutate(Mutation::AddApplication(path.into()))
                .unwrap();
        }
        let mutation = preflight(
            &mut backend,
            Mutation::Applications {
                paths: vec![path.into()],
                action: xield::model::Action::Block,
            },
        )
        .unwrap();
        backend.mutate(mutation).unwrap();
        let snapshot = backend.snapshot().unwrap();
        assert!(
            snapshot
                .applications
                .iter()
                .any(|app| app.path == path && app.blocked)
        );
        assert!(
            snapshot
                .applications
                .iter()
                .any(|app| app.path == helper && !app.blocked)
        );
        let mutation = preflight(&mut backend, Mutation::RemoveApplication(path.into())).unwrap();
        backend.mutate(mutation).unwrap();
        let snapshot = backend.snapshot().unwrap();
        assert!(!snapshot.applications.iter().any(|app| app.path == path));
        assert!(snapshot.applications.iter().any(|app| app.path == helper));
    }
}
