mod common;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use xield::{
    app::{App, Effect, Popup, ProfileOperation, ProfileOutcome},
    model::Profile,
    profile,
    ui::{self, HitMap, Theme},
};

fn key(app: &mut App, code: KeyCode) -> Effect {
    app.handle(KeyEvent::new(code, KeyModifiers::NONE))
}
fn open(app: &mut App, entries: Vec<String>) {
    assert!(matches!(
        key(app, KeyCode::Char('p')).profile,
        Some(ProfileOperation::List)
    ));
    assert!(app.busy);
    app.profiles_finished(common::snapshot(), ProfileOutcome::Listed(entries));
    assert!(!app.busy);
}
fn prepared() -> profile::Prepared {
    let before = common::snapshot();
    let mut proposed = Profile::from_snapshot(&before);
    proposed.applications.clear();
    proposed.network_rules.clear();
    proposed.firewall.as_mut().unwrap().stealth = true;
    profile::prepare(&proposed, &before).unwrap()
}
fn review(app: &mut App) {
    assert!(
        matches!(key(app, KeyCode::Enter).profile, Some(ProfileOperation::Prepare(name)) if name == "Home")
    );
    app.profiles_finished(
        common::snapshot(),
        ProfileOutcome::Prepared {
            name: "Home".into(),
            prepared: Box::new(prepared()),
        },
    );
    assert!(matches!(app.popup, Some(Popup::Confirm { .. })));
}
fn render(app: &App, width: u16, height: u16) -> (HitMap, Buffer) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let mut hits = None;
    terminal
        .draw(|frame| {
            hits = Some(ui::draw_interactive(
                frame,
                app,
                Theme::Dark,
                &mut ui::State::default(),
            ))
        })
        .unwrap();
    (hits.unwrap(), terminal.backend().buffer().clone())
}
fn point(buffer: &Buffer, label: &str) -> (u16, u16) {
    for row in 0..buffer.area.height {
        let text = (0..buffer.area.width)
            .map(|column| buffer[(column, row)].symbol())
            .collect::<String>();
        if let Some(byte) = text.find(label) {
            return (text[..byte].chars().count() as u16, row);
        }
    }
    panic!("missing rendered label {label}");
}
fn click(app: &mut App, label: &str, width: u16, height: u16) -> Effect {
    let (hits, buffer) = render(app, width, height);
    let (column, row) = point(&buffer, label);
    let action = hits
        .action(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
        .unwrap();
    app.handle_mouse(action)
}

#[test]
fn profile_navigation_review_and_apply_are_separate_actions() {
    let mut app = App::new(common::snapshot());
    open(&mut app, vec!["Home".into(), "Work".into()]);
    key(&mut app, KeyCode::Down);
    assert!(
        matches!(key(&mut app, KeyCode::Enter).profile, Some(ProfileOperation::Prepare(name)) if name == "Work")
    );
    app.profiles_finished(
        common::snapshot(),
        ProfileOutcome::Prepared {
            name: "Work".into(),
            prepared: Box::new(prepared()),
        },
    );
    assert!(!app.busy);
    let apply = key(&mut app, KeyCode::Enter);
    assert!(matches!(apply.profile, Some(ProfileOperation::Apply(_))));
    assert!(app.busy);
    assert!(apply.mutation.is_none());
    assert!(!apply.authenticate);
}

#[test]
fn dismissed_async_list_or_review_never_reopens_a_modal() {
    let mut app = App::new(common::snapshot());
    key(&mut app, KeyCode::Char('p'));
    key(&mut app, KeyCode::Esc);
    app.profiles_finished(
        common::snapshot(),
        ProfileOutcome::Listed(vec!["Home".into()]),
    );
    assert!(app.popup.is_none());
    open(&mut app, vec!["Home".into()]);
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Esc);
    app.profiles_finished(
        common::snapshot(),
        ProfileOutcome::Prepared {
            name: "Home".into(),
            prepared: Box::new(prepared()),
        },
    );
    assert!(app.popup.is_none());
    assert!(!app.busy);
    assert!(key(&mut app, KeyCode::Enter).profile.is_none());
}

#[test]
fn cancelling_profile_confirmation_emits_no_operation() {
    let mut app = App::new(common::snapshot());
    open(&mut app, vec!["Home".into()]);
    review(&mut app);
    assert!(key(&mut app, KeyCode::Esc).profile.is_none());
    assert!(app.popup.is_none());
    assert!(!app.busy);
}

#[test]
fn exporting_requires_an_explicit_bounded_name_and_does_not_apply() {
    let mut app = App::new(common::snapshot());
    open(&mut app, vec![]);
    assert!(key(&mut app, KeyCode::Enter).profile.is_none());
    key(&mut app, KeyCode::Char('e'));
    assert!(matches!(app.popup, Some(Popup::ProfileName { .. })));
    assert!(key(&mut app, KeyCode::Enter).profile.is_none());
    for character in "Home".chars() {
        key(&mut app, KeyCode::Char(character));
    }
    assert!(
        matches!(key(&mut app, KeyCode::Enter).profile, Some(ProfileOperation::Export(name)) if name == "Home")
    );
    app.profiles_finished(
        common::snapshot(),
        ProfileOutcome::Exported {
            name: "Home".into(),
            entries: vec!["Home".into()],
        },
    );
    assert!(!app.busy);
    assert!(app.notice.as_ref().is_some_and(|notice| !notice.error));
    assert!(matches!(app.popup, Some(Popup::Profiles { .. })));
}

#[test]
fn mouse_profile_review_and_confirmation_follow_rendered_geometry() {
    for (width, height) in [(50, 17), (80, 24), (120, 34)] {
        let mut app = App::new(common::snapshot());
        let (_, underlay) = render(&app, width, height);
        let background = point(&underlay, "Activity");
        open(&mut app, vec!["Home".into(), "Work".into()]);
        assert!(click(&mut app, "Home", width, height).profile.is_none());
        assert!(
            matches!(click(&mut app, "[Enter Review]", width,height).profile, Some(ProfileOperation::Prepare(name)) if name == "Home")
        );
        app.profiles_finished(
            common::snapshot(),
            ProfileOutcome::Prepared {
                name: "Home".into(),
                prepared: Box::new(prepared()),
            },
        );
        let (hits, _) = render(&app, width, height);
        let (column, row) = background;
        assert!(
            hits.action(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE
            })
            .is_none()
        );
        assert!(matches!(
            click(&mut app, "[Enter Apply]", width, height).profile,
            Some(ProfileOperation::Apply(_))
        ));
    }
}

#[test]
fn scrolled_profile_rows_keep_their_actual_targets() {
    for (width, height) in [(50, 17), (80, 24)] {
        let mut app = App::new(common::snapshot());
        open(
            &mut app,
            (0..30).map(|index| format!("Profile {index:02}")).collect(),
        );
        key(&mut app, KeyCode::End);
        assert!(
            click(&mut app, "Profile 28", width, height)
                .profile
                .is_none()
        );
        assert!(
            matches!(click(&mut app, "[Enter Review]", width, height).profile,
            Some(ProfileOperation::Prepare(name)) if name == "Profile 28")
        );
    }
}
