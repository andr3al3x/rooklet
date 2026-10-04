use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use xield::{
    app::{App, MouseAction, Popup},
    backend::Backend,
    model::{Action, Application, Mutation, Snapshot},
    permissions::{IncomingState, Index},
    ui::{self, Theme},
};
fn key(app: &mut App, code: KeyCode) -> xield::app::Effect {
    app.handle(KeyEvent::new(code, KeyModifiers::NONE))
}
fn fixture() -> Snapshot {
    let mut snapshot = Backend::new(true).unwrap().snapshot().unwrap();
    snapshot.activity.truncate(1);
    let identity = snapshot.activity[0].identities[0].clone();
    let mut helper = identity.clone();
    helper.pid += 10;
    helper.path = format!(
        "{}/Contents/Helpers/helper",
        identity.bundle_path.as_deref().unwrap()
    );
    snapshot.activity[0].identities.push(helper.clone());
    snapshot.applications = vec![
        Application {
            path: identity.path,
            name: "main executable".into(),
            blocked: false,
        },
        Application {
            path: helper.path,
            name: "helper".into(),
            blocked: true,
        },
    ];
    snapshot
}
fn text(app: &App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| ui::draw(frame, app, Theme::Dark))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}
#[test]
fn grouped_executables_show_mixed_and_confirm_all_exact_entries() {
    let mut app = App::new(fixture());
    let resolution = app.incoming(&app.snapshot.activity[0]);
    assert_eq!(resolution.state, IncomingState::Mixed);
    assert_eq!(resolution.paths.len(), 2);
    assert!(text(&app, 120, 34).contains("Mixed"));
    assert!(key(&mut app, KeyCode::Char('b')).mutation.is_none());
    if let Some(Popup::Confirm { body, .. }) = &app.popup {
        for path in &resolution.paths {
            assert!(body.contains(path));
        }
    } else {
        panic!("missing confirmation");
    }
    // A later refresh must not change the captured confirmation scope.
    let mut next = app.snapshot.clone();
    next.applications.clear();
    app.update(next, false);
    assert!(
        matches!(key(&mut app,KeyCode::Enter).mutation,Some(Mutation::Applications{paths,action:Action::Block}) if paths==resolution.paths)
    );
}
#[test]
fn frozen_activity_uses_current_permissions_and_unavailable_never_emits_action() {
    let mut app = App::new(fixture());
    key(&mut app, KeyCode::Char(' '));
    let mut next = app.snapshot.clone();
    for entry in &mut next.applications {
        entry.blocked = true;
    }
    app.update(next, false);
    assert_eq!(
        app.incoming(&app.snapshot.activity[0]).state,
        IncomingState::Block
    );
    app.snapshot.applications_available = false;
    assert_eq!(
        app.incoming(&app.snapshot.activity[0]).state,
        IncomingState::Unknown
    );
    assert!(key(&mut app, KeyCode::Char('a')).mutation.is_none());
    assert!(app.popup.is_none());
}
#[test]
fn demo_multi_target_preflight_keeps_every_entry_unchanged_on_stale_target() {
    let mut backend = Backend::new(true).unwrap();
    let before = backend.snapshot().unwrap().applications;
    assert!(
        backend
            .mutate(Mutation::Applications {
                paths: vec![before[0].path.clone(), "/missing/stale".into()],
                action: Action::Block
            })
            .is_err()
    );
    let after = backend.snapshot().unwrap().applications;
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(after).unwrap()
    );
}
#[test]
fn on_disk_bundle_resolution_handles_nested_helpers_aliases_and_unrelated_names() {
    let directory = tempfile::tempdir().unwrap();
    let bundle = directory.path().join("Example.app");
    let helper = bundle.join("Contents/Helpers/Nested.app/Contents/MacOS/helper");
    std::fs::create_dir_all(helper.parent().unwrap()).unwrap();
    std::fs::write(&helper, "fixture").unwrap();
    std::fs::write(bundle.join("Contents/Info.plist"), "fixture").unwrap();
    std::fs::write(
        bundle.join("Contents/Helpers/Nested.app/Contents/Info.plist"),
        "fixture",
    )
    .unwrap();
    let alias = directory.path().join("alias");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&helper, &alias).unwrap();
    #[cfg(not(unix))]
    std::fs::copy(&helper, &alias).unwrap();
    let mut snapshot = fixture();
    snapshot.activity[0].path = Some(bundle.to_string_lossy().into_owned());
    snapshot.activity[0].identities.clear();
    snapshot.applications = vec![Application {
        path: helper.to_string_lossy().into_owned(),
        name: "different name".into(),
        blocked: false,
    }];
    #[cfg(unix)]
    snapshot.applications.push(Application {
        path: alias.to_string_lossy().into_owned(),
        name: "alias".into(),
        blocked: true,
    });
    let nested_bundle = bundle.join("Contents/Helpers/Nested.app");
    snapshot.applications.push(Application {
        path: nested_bundle.to_string_lossy().into_owned(),
        name: "nested bundle".into(),
        blocked: true,
    });
    let resolution = Index::new(&snapshot).activity(&snapshot.activity[0]);
    #[cfg(unix)]
    assert_eq!(resolution.state, IncomingState::Mixed);
    #[cfg(not(unix))]
    assert_eq!(resolution.state, IncomingState::Mixed);
    assert!(
        resolution
            .paths
            .contains(&helper.to_string_lossy().into_owned())
    );
    assert!(
        resolution
            .paths
            .contains(&nested_bundle.to_string_lossy().into_owned())
    );
    snapshot.activity[0].path = Some(format!("{}-lookalike", bundle.display()));
    assert_eq!(
        Index::new(&snapshot).activity(&snapshot.activity[0]).state,
        IncomingState::Unlisted
    );
}
#[test]
fn long_confirmations_scroll_to_last_target_and_back_at_multiple_sizes() {
    let mut app = App::new(fixture());
    key(&mut app, KeyCode::Char('b'));
    if let Some(Popup::Confirm { body, .. }) = &mut app.popup {
        *body = (0..80)
            .map(|i| format!("/fixture/App.app/Contents/MacOS/executable-{i:03}"))
            .collect::<Vec<_>>()
            .join("\n");
    }
    for (width, height) in [(50, 17), (80, 24), (120, 34)] {
        key(&mut app, KeyCode::End);
        assert!(text(&app, width, height).contains("079"));
        assert!(text(&app, width, height).contains("[Esc Cancel]"));
        app.handle_mouse(MouseAction::DialogScroll(-3));
        assert!(text(&app, width, height).contains("076"));
        key(&mut app, KeyCode::Home);
        assert!(text(&app, width, height).contains("000"));
        key(&mut app, KeyCode::Up);
        assert!(text(&app, width, height).contains("000"));
    }
    assert!(key(&mut app, KeyCode::Esc).mutation.is_none());
}

#[cfg(unix)]
#[test]
fn standalone_executable_alias_matches_identity_without_rewriting_captured_target() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("program");
    std::fs::write(&executable, "fixture").unwrap();
    let alias = directory.path().join("registered-alias");
    std::os::unix::fs::symlink(&executable, &alias).unwrap();
    let mut snapshot = fixture();
    snapshot.activity[0].path = Some(executable.to_string_lossy().into_owned());
    snapshot.activity[0].identities.clear();
    snapshot.applications = vec![Application {
        path: alias.to_string_lossy().into_owned(),
        name: "alias".into(),
        blocked: true,
    }];
    let resolution = Index::new(&snapshot).activity(&snapshot.activity[0]);
    assert_eq!(resolution.state, IncomingState::Block);
    assert_eq!(resolution.paths, vec![alias.to_string_lossy().into_owned()]);
}
