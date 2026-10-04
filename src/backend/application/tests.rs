use super::*;
use std::{fs, os::unix::fs::PermissionsExt};

fn bundle(directory: &Path) -> (String, String) {
    let app = directory.join("Example App.app");
    let executable = app.join("Contents/MacOS/Declared Name");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, b"fixture").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(app.join("Contents/Info.plist"), b"injected plist fixture").unwrap();
    (
        fs::canonicalize(app).unwrap().to_str().unwrap().to_owned(),
        fs::canonicalize(executable)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned(),
    )
}

#[test]
fn bundle_registration_uses_declared_executable_and_preserves_spaces() {
    let directory = tempfile::tempdir().unwrap();
    let (app, executable) = bundle(directory.path());
    let resolved = registration_path_with(&app, &mut |plist| {
        assert_eq!(plist, format!("{app}/Contents/Info.plist"));
        Ok("Declared Name\n".into())
    })
    .unwrap();
    assert_eq!(resolved, executable);
    assert_eq!(
        registration_path_with(&executable, &mut |_| panic!(
            "file input must not query plist"
        ))
        .unwrap(),
        executable
    );
}

#[test]
fn executable_alias_resolves_actual_path() {
    let directory = tempfile::tempdir().unwrap();
    let (_, executable) = bundle(directory.path());
    let alias = directory.path().join("alias");
    std::os::unix::fs::symlink(&executable, &alias).unwrap();
    assert_eq!(
        registration_path_with(alias.to_str().unwrap(), &mut |_| panic!("not a bundle")).unwrap(),
        executable
    );
}

#[test]
fn missing_invalid_and_non_executable_bundle_declarations_fail_preflight() {
    let directory = tempfile::tempdir().unwrap();
    let (app, executable) = bundle(directory.path());
    for name in [
        "",
        "../Declared Name",
        "nested/main",
        ".",
        "..",
        "name\nextra",
        "Declared Name\n\n",
        "Missing",
    ] {
        assert!(
            registration_path_with(&app, &mut |_| Ok(name.into())).is_err(),
            "{name:?}"
        );
    }
    assert!(
        registration_path_with(&app, &mut |_| anyhow::bail!("missing CFBundleExecutable")).is_err()
    );
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(registration_path_with(&app, &mut |_| Ok("Declared Name".into())).is_err());
}

#[test]
fn absent_empty_and_oversized_plist_never_reaches_parser() {
    let directory = tempfile::tempdir().unwrap();
    let (app, _) = bundle(directory.path());
    let plist = Path::new(&app).join("Contents/Info.plist");
    for size in [0, MAX_PLIST_BYTES + 1] {
        let file = fs::File::create(&plist).unwrap();
        file.set_len(size).unwrap();
        assert!(
            registration_path_with(&app, &mut |_| panic!("invalid size must not query plist"))
                .is_err()
        );
    }
    fs::remove_file(&plist).unwrap();
    assert!(
        registration_path_with(&app, &mut |_| panic!("missing plist must not be queried")).is_err()
    );
}

#[test]
fn lexical_normalization_and_target_bounds_are_strict() {
    for path in [
        "/Apps/./Example.app",
        "/Apps//Example.app",
        "/Apps/Example.app/",
        "//Apps/Example.app",
    ] {
        assert!(validate_application_path(path, false).is_err(), "{path}");
    }
    assert!(validate_application_targets(&[], false).is_err());
    assert!(validate_application_targets(&vec!["/one".into(); 257], false).is_err());
    assert!(validate_application_targets(&["/one".into(), "/one".into()], false).is_err());
    assert!(validate_application_targets(&["/one".into(), "/two".into()], false).is_ok());
}

#[cfg(target_os = "macos")]
#[test]
fn actual_plutil_accepts_string_metadata_and_rejects_malformed_or_wrong_types() {
    let directory = tempfile::tempdir().unwrap();
    let (app, executable) = bundle(directory.path());
    let plist = Path::new(&app).join("Contents/Info.plist");
    let xml = |entry: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict>{entry}</dict></plist>"
        )
    };
    fs::write(
        &plist,
        xml("<key>CFBundleExecutable</key><string>Declared Name</string>"),
    )
    .unwrap();
    assert_eq!(registration_path(&app).unwrap(), executable);
    for contents in [
        xml(""),
        xml("<key>CFBundleExecutable</key><integer>42</integer>"),
        "malformed plist".into(),
    ] {
        fs::write(&plist, contents).unwrap();
        assert!(registration_path(&app).is_err());
    }
}
