//! Resolve CLI input to exact ALF registrations before authentication.
use anyhow::{Result, ensure};
use rooklet_core::{
    application::validate_path,
    model::{Application, Mutation},
};
use rooklet_macos::{
    application::{registration_path, validate_existing_path},
    backend::Backend,
};

pub(super) fn resolve_path(
    path: &str,
    current: &[Application],
    resolve: &mut impl FnMut(&str) -> Result<String>,
) -> Result<String> {
    validate_path(path)?;
    if current.iter().any(|app| app.path == path) {
        return Ok(path.to_owned());
    }
    resolve(path)
}

fn registered_path(
    path: &str,
    current: &[Application],
    resolve: &mut impl FnMut(&str) -> Result<String>,
) -> Result<String> {
    let path = resolve_path(path, current, resolve)?;
    ensure!(
        current.iter().any(|app| app.path == path),
        "application is not registered at {path:?}; add it first"
    );
    Ok(path)
}

pub(super) fn preflight(backend: &mut Backend, mutation: Mutation) -> Result<Mutation> {
    match mutation {
        Mutation::AddApplication(path) => {
            validate_path(&path)?;
            let path = registration_path(&path)?;
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
                    let path =
                        registered_path(path, &current.applications, &mut registration_path)?;
                    validate_existing_path(&path)?;
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
        let path = registered_path("/Applications/App.app", &current, &mut |_| {
            panic!("exact registration must not resolve")
        })
        .unwrap();
        assert_eq!(path, "/Applications/App.app");
    }

    #[test]
    fn bundle_input_selects_only_its_registered_executable() {
        let current = [entry("/real/App"), entry("/real/Helper")];
        let path = registered_path("/Applications/App.app", &current, &mut |path| {
            assert_eq!(path, "/Applications/App.app");
            Ok("/real/App".into())
        })
        .unwrap();
        assert_eq!(path, "/real/App");
        assert!(
            registered_path("/Applications/Other.app", &current, &mut |_| {
                Ok("/real/Other".into())
            })
            .is_err()
        );
    }
}
