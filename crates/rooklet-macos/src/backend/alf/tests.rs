use super::{
    FIREWALL,
    operations::{self, Runner},
};
use anyhow::Result;
use rooklet_core::model::Action;
use std::{collections::VecDeque, fs, os::unix::fs::PermissionsExt};

struct Call {
    program: &'static str,
    args: Vec<String>,
    privileged: bool,
    output: Result<String>,
}

#[derive(Default)]
struct Fake {
    calls: VecDeque<Call>,
}
impl Fake {
    fn expect(&mut self, args: &[&str], privileged: bool, output: Result<String>) {
        self.calls.push_back(Call {
            program: FIREWALL,
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            privileged,
            output,
        });
    }
}
impl Runner for Fake {
    fn run(&mut self, program: &str, args: &[&str], privileged: bool) -> Result<String> {
        let call = self.calls.pop_front().expect("unexpected command");
        assert_eq!(program, call.program);
        assert_eq!(args, call.args);
        assert_eq!(privileged, call.privileged);
        call.output
    }
}

fn paths(directory: &std::path::Path) -> Vec<String> {
    ["main", "helper"]
        .iter()
        .map(|name| {
            let path = directory.join(name);
            fs::write(&path, b"fixture").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            fs::canonicalize(path).unwrap().to_str().unwrap().to_owned()
        })
        .collect()
}

fn list(paths: &[String], blocked: &[bool]) -> String {
    let mut output = format!("Total number of apps = {}\n", paths.len());
    for (index, (path, blocked)) in paths.iter().zip(blocked).enumerate() {
        output.push_str(&format!(
            "{} : {path}\n( {} incoming connections )\n",
            index + 1,
            if *blocked { "Block" } else { "Allow" }
        ));
    }
    output
}

#[test]
fn grouped_action_preflights_all_targets_and_verifies_each_exact_registration() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let mut fake = Fake::default();
    fake.expect(&["--listapps"], false, Ok(list(&paths, &[false, false])));
    for (index, path) in paths.iter().enumerate() {
        fake.expect(&["--blockapp", path], true, Ok(String::new()));
        fake.expect(
            &["--getappblocked", path],
            false,
            Ok(format!("Incoming connection to {path} is blocked.\n")),
        );
        fake.expect(
            &["--listapps"],
            false,
            Ok(list(&paths, &[true, index == 1])),
        );
    }
    operations::set_applications(&paths, Action::Block, &mut fake).unwrap();
    assert!(fake.calls.is_empty());
}

#[test]
fn invalid_duplicate_missing_or_unregistered_targets_never_mutate() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    for invalid in [
        vec![],
        vec![paths[0].clone(); 257],
        vec![paths[0].clone(); 2],
        vec![paths[0].clone(), "/nonexistent/rooklet-test-target".into()],
    ] {
        assert!(
            operations::set_applications(&invalid, Action::Block, &mut Fake::default()).is_err()
        );
    }
    let mut fake = Fake::default();
    fake.expect(&["--listapps"], false, Ok(list(&paths[..1], &[false])));
    assert!(operations::set_applications(&paths, Action::Block, &mut fake).is_err());
    assert!(fake.calls.is_empty());
}

#[test]
fn later_failure_reports_changed_entries_and_unverified_target() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let mut fake = Fake::default();
    fake.expect(&["--listapps"], false, Ok(list(&paths, &[false, false])));
    fake.expect(&["--blockapp", &paths[0]], true, Ok(String::new()));
    fake.expect(
        &["--getappblocked", &paths[0]],
        false,
        Ok(format!("Incoming connection to {} is blocked.", paths[0])),
    );
    fake.expect(&["--listapps"], false, Ok(list(&paths, &[true, false])));
    fake.expect(
        &["--blockapp", &paths[1]],
        true,
        Err(anyhow::anyhow!("injected command failure")),
    );
    let error = operations::set_applications(&paths, Action::Block, &mut fake)
        .unwrap_err()
        .to_string();
    assert!(error.contains(&paths[0]) && error.contains(&paths[1]));
    assert!(
        error.contains("previously verified changed entries")
            && error.contains("this target may have changed")
    );
    assert!(error.contains("ALF changes are not atomic"));
    assert!(fake.calls.is_empty());
}

#[test]
fn mismatched_getappblocked_path_does_not_claim_success() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let mut fake = Fake::default();
    fake.expect(&["--listapps"], false, Ok(list(&paths, &[true, true])));
    fake.expect(&["--unblockapp", &paths[0]], true, Ok(String::new()));
    fake.expect(
        &["--getappblocked", &paths[0]],
        false,
        Ok(format!("Incoming connection to {} is permitted.", paths[1])),
    );
    assert!(operations::set_applications(&paths, Action::Allow, &mut fake).is_err());
    assert!(fake.calls.is_empty());
}

#[test]
fn successful_command_with_unchanged_registration_is_partial_failure() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let mut fake = Fake::default();
    fake.expect(&["--listapps"], false, Ok(list(&paths, &[false, false])));
    fake.expect(&["--blockapp", &paths[0]], true, Ok(String::new()));
    fake.expect(
        &["--getappblocked", &paths[0]],
        false,
        Ok(format!("Incoming connection to {} is blocked.", paths[0])),
    );
    fake.expect(&["--listapps"], false, Ok(list(&paths, &[false, false])));
    let error = operations::set_applications(&paths, Action::Block, &mut fake)
        .unwrap_err()
        .to_string();
    assert!(error.contains("1 remaining entries were not attempted"));
    assert!(fake.calls.is_empty());
}

#[test]
fn bundle_add_registers_and_verifies_declared_executable_instead_of_bundle_path() {
    let directory = tempfile::tempdir().unwrap();
    let app = directory.path().join("Example.app");
    fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    let paths = paths(&app.join("Contents/MacOS"));
    fs::write(app.join("Contents/Info.plist"), b"injected plist fixture").unwrap();
    let app = fs::canonicalize(app).unwrap();
    let app = app.to_str().unwrap();
    let mut fake = Fake::default();
    fake.calls.push_back(Call {
        program: "/usr/bin/plutil",
        args: [
            "-extract",
            "CFBundleExecutable",
            "raw",
            "-expect",
            "string",
            "-o",
            "-",
            &format!("{app}/Contents/Info.plist"),
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect(),
        privileged: false,
        output: Ok("main\n".into()),
    });
    fake.expect(&["--add", &paths[0]], true, Ok(String::new()));
    fake.expect(&["--listapps"], false, Ok(list(&paths[..1], &[false])));
    operations::add_application(app, &mut fake).unwrap();
    assert!(fake.calls.is_empty());
}

#[test]
fn add_does_not_accept_unrelated_helper_registration() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let mut fake = Fake::default();
    fake.expect(&["--add", &paths[0]], true, Ok(String::new()));
    fake.expect(&["--listapps"], false, Ok(list(&paths[1..], &[false])));
    assert!(operations::add_application(&paths[0], &mut fake).is_err());
    assert!(fake.calls.is_empty());
}

#[test]
fn registered_paths_preserve_trailing_filename_spaces() {
    let output = "Total number of apps = 1 \n1 : /Applications/Example.app/Contents/MacOS/main  \n             ( Allow incoming connections )\n";
    let apps = crate::backend::parser::parse_applications(output).unwrap();
    assert_eq!(
        apps[0].path,
        "/Applications/Example.app/Contents/MacOS/main "
    );
    let path = &apps[0].path;
    assert!(
        !crate::backend::parser::parse_app_blocked(
            &format!("Incoming connection to {path} is permitted.\n"),
            path
        )
        .unwrap()
    );
}

#[test]
fn removal_rejects_unregistered_path_and_verifies_exact_captured_entry() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let mut fake = Fake::default();
    fake.expect(&["--listapps"], false, Ok(list(&paths[1..], &[false])));
    assert!(operations::remove_application(&paths[0], &mut fake).is_err());
    assert!(fake.calls.is_empty());
    fake.expect(&["--listapps"], false, Ok(list(&paths, &[false, false])));
    fake.expect(&["--remove", &paths[0]], true, Ok(String::new()));
    fake.expect(&["--listapps"], false, Ok(list(&paths[1..], &[false])));
    operations::remove_application(&paths[0], &mut fake).unwrap();
    assert!(fake.calls.is_empty());
}
