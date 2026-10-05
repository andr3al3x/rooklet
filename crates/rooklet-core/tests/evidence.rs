use rooklet_core::{
    model::{Application, ProcessActivity, Snapshot},
    permissions::{IncomingState, Index, Paths},
    process::{MAX_TARGETS, ProcessIdentity, members_for},
    resources::{ProcessReading, ReadingState, Resources, Usage},
};

fn identity(pid: u32, path: &str, bundle: Option<&str>) -> ProcessIdentity {
    ProcessIdentity {
        pid,
        uid: 501,
        parent_pid: 1,
        start_sec: 100,
        start_usec: 0,
        pid_version: 1,
        path: path.into(),
        bundle_path: bundle.map(str::to_owned),
    }
}
fn activity(path: &str, identities: Vec<ProcessIdentity>) -> ProcessActivity {
    ProcessActivity {
        pid: identities.first().map_or(10, |identity| identity.pid),
        name: "App".into(),
        path: Some(path.into()),
        identities,
        bytes_in: 0,
        bytes_out: 0,
        rate_in: 0,
        rate_out: 0,
        connections: vec![],
    }
}
fn registration(path: &str, blocked: bool) -> Application {
    Application {
        path: path.into(),
        name: "Same arbitrary label".into(),
        blocked,
    }
}

#[test]
fn canonical_aliases_retain_exact_registered_mutation_targets() {
    let mut snapshot = Snapshot {
        applications_available: true,
        ..Default::default()
    };
    snapshot.applications = vec![registration("/alias", true)];
    snapshot
        .permission_paths
        .record_registration("/alias".into(), "/program".into(), None);
    snapshot
        .permission_paths
        .record_activity("/program".into(), "/program".into());
    let result = Index::new(&snapshot).activity(&activity("/program", vec![]));
    assert_eq!(result.state, IncomingState::Block);
    assert_eq!(result.paths, ["/alias"]);
    assert_eq!(
        Index::new(&snapshot)
            .activity(&activity("/program-lookalike", vec![]))
            .state,
        IncomingState::Unlisted
    );
    snapshot.applications_available = false;
    assert_eq!(
        Index::new(&snapshot)
            .activity(&activity("/program", vec![]))
            .state,
        IncomingState::Unknown
    );
}

#[test]
fn verified_bundles_include_helpers_and_ignore_display_names() {
    let mut snapshot = Snapshot {
        applications_available: true,
        ..Default::default()
    };
    snapshot.applications = vec![
        registration("/main", false),
        registration("/helper", true),
        registration("/unrelated", true),
    ];
    for path in ["/main", "/helper"] {
        snapshot.permission_paths.record_registration(
            path.into(),
            path.into(),
            Some("/App.app".into()),
        );
    }
    let result = Index::new(&snapshot).activity(&activity("/App.app", vec![]));
    assert_eq!(result.state, IncomingState::Mixed);
    assert_eq!(result.paths, ["/helper", "/main"]);
}

#[test]
fn captured_identity_bundle_membership_works_without_filesystem_evidence() {
    let mut snapshot = Snapshot {
        applications_available: true,
        ..Default::default()
    };
    snapshot.applications = vec![registration("/helper", false)];
    snapshot.activity = vec![activity(
        "/App.app",
        vec![identity(10, "/helper", Some("/App.app"))],
    )];
    let result = Index::new(&snapshot).activity(&snapshot.activity[0]);
    assert_eq!(result.state, IncomingState::Allow);
    assert_eq!(result.paths, ["/helper"]);
}

#[test]
fn frozen_activity_preserves_identity_while_registration_evidence_updates() {
    let mut previous = Paths::default();
    previous.record_activity("/alias".into(), "/original".into());
    let mut current = Paths::default();
    current.record_activity("/alias".into(), "/replacement".into());
    current.record_registration("/registration".into(), "/original".into(), None);
    current.preserve_activity(&previous);
    let snapshot = Snapshot {
        applications_available: true,
        applications: vec![registration("/registration", true)],
        permission_paths: current,
        ..Default::default()
    };
    let result = Index::new(&snapshot).activity(&activity("/alias", vec![]));
    assert_eq!(result.state, IncomingState::Block);
    assert_eq!(result.paths, ["/registration"]);
}

#[test]
fn grouped_targets_require_captured_membership_and_same_owner() {
    let selected = identity(10, "/main", Some("/App.app"));
    let helper = identity(11, "/helper", Some("/App.app"));
    let mut foreign = identity(12, "/foreign", Some("/App.app"));
    foreign.uid = 0;
    let unrelated = identity(13, "/other", Some("/Other.app"));
    let targets = vec![helper.clone(), selected.clone(), foreign, unrelated];
    assert_eq!(members_for(&targets, 10), [selected, helper]);
    assert!(members_for(&targets, 999).is_empty());

    let unbundled = identity(20, "/standalone", None);
    let same_path = identity(21, "/standalone", None);
    assert_eq!(
        members_for(&[unbundled.clone(), same_path], 20),
        [unbundled]
    );

    let mut members: Vec<_> = (1..=MAX_TARGETS as u32)
        .map(|pid| identity(pid, "/helper", Some("/App.app")))
        .collect();
    assert_eq!(members_for(&members, 1), members);
    members.push(identity(
        MAX_TARGETS as u32 + 1,
        "/helper",
        Some("/App.app"),
    ));
    assert_eq!(members_for(&members, 1), members);
}

#[test]
fn resource_readings_reject_reused_or_exec_changed_process_identities() {
    let captured = identity(10, "/program", None);
    let reading = ProcessReading {
        identity: captured.clone(),
        cpu_percent: None,
        memory_bytes: Some(1024),
        read_per_sec: None,
        write_per_sec: None,
        state: ReadingState::WarmingUp,
        age_ms: Some(0),
    };
    let usage = Usage {
        process_count: 1,
        sampled_count: 1,
        cpu_percent: None,
        memory_bytes: Some(1024),
        read_per_sec: None,
        write_per_sec: None,
        state: ReadingState::WarmingUp,
        age_ms: Some(0),
        processes: vec![reading],
    };
    let mut resources = Resources::default();
    resources.groups.insert("pid:10".into(), usage);
    assert!(
        resources
            .for_activity(&activity("/program", vec![captured.clone()]))
            .is_some()
    );
    let mut reused = captured.clone();
    reused.start_sec += 1;
    assert!(
        resources
            .for_activity(&activity("/program", vec![reused]))
            .is_none()
    );
    let mut executed = captured;
    executed.pid_version += 1;
    assert!(
        resources
            .for_activity(&activity("/program", vec![executed]))
            .is_none()
    );
    assert!(
        resources
            .for_activity(&activity("/program", vec![]))
            .is_none()
    );
}
