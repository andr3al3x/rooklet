mod common;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use rooklet::{
    app::{App, ConfirmedAction, Effect, Popup, View},
    process::{SignalFailure, TerminationMode, TerminationReport, TerminationRequest},
    ui::{self, Theme},
};

fn key(app: &mut App, code: KeyCode) -> Effect {
    app.handle(KeyEvent::new(code, KeyModifiers::NONE))
}
fn fixture_app() -> App {
    App::new(common::snapshot())
}
fn proposed(app: &App) -> &TerminationRequest {
    match app.popup.as_ref().unwrap() {
        Popup::Confirm {
            action: ConfirmedAction::Terminate(request),
            ..
        } => request,
        _ => panic!("expected termination confirmation"),
    }
}

#[test]
fn modes_require_confirmation_and_cancellation_sends_nothing() {
    let mut app = fixture_app();
    for (code, mode) in [
        ('x', TerminationMode::Terminate),
        ('X', TerminationMode::ForceKill),
    ] {
        let effect = key(&mut app, KeyCode::Char(code));
        assert!(effect.terminate.is_none());
        assert!(!app.busy);
        assert_eq!(proposed(&app).mode, mode);
        assert_eq!(proposed(&app).targets.len(), 2);
        let effect = key(&mut app, KeyCode::Esc);
        assert!(effect.terminate.is_none());
        assert!(app.popup.is_none());
    }
}

#[test]
fn peer_selection_targets_its_app_and_preserves_confirmed_identity() {
    let mut app = fixture_app();
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Char('x'));
    let original = proposed(&app).clone();
    let mut snapshot = common::snapshot();
    snapshot.activity[0].identities[0].pid_version += 1;
    app.update(snapshot, false);
    let effect = key(&mut app, KeyCode::Enter);
    assert_eq!(effect.terminate, Some(original));
    assert!(effect.mutation.is_none());
    assert!(!effect.authenticate);
    assert!(app.busy);
    assert!(key(&mut app, KeyCode::Char('X')).terminate.is_none());
    assert!(app.popup.is_none());
}

#[test]
fn unverified_rows_and_non_activity_views_do_not_propose_termination() {
    let mut app = fixture_app();
    app.snapshot.activity[0].identities.clear();
    key(&mut app, KeyCode::Char('x'));
    assert!(app.popup.is_none());
    assert!(app.notice.as_ref().unwrap().error);
    for view in [View::Applications, View::Settings, View::Network] {
        app.view = view;
        key(&mut app, KeyCode::Char('X'));
        assert!(app.popup.is_none());
    }
    app.view = View::Activity;
    key(&mut app, KeyCode::Char('/'));
    key(&mut app, KeyCode::Char('x'));
    assert!(app.popup.is_none());
    assert_eq!(app.filter(), "x");
}

#[test]
fn completed_signal_report_preserves_observed_rows_until_refresh() {
    let mut app = fixture_app();
    key(&mut app, KeyCode::Char('x'));
    let request = key(&mut app, KeyCode::Enter).terminate.unwrap();
    let observed = app.snapshot.clone();
    app.termination_finished(
        observed,
        &TerminationReport {
            attempted: request.targets.len(),
            delivered: request.targets.iter().map(|target| target.pid).collect(),
            failures: Vec::new(),
        },
    );
    assert!(!app.busy);
    assert_eq!(app.snapshot.activity[0].identities, request.targets);
    let notice = app.notice.unwrap();
    assert!(!notice.error);
    assert!(notice.text.contains("Signal delivered to 2 processes"));
    assert!(notice.text.contains("refresh"));
    assert!(!notice.text.contains("terminated"));
}

#[test]
fn partial_result_reports_delivery_without_claiming_exit() {
    let mut app = fixture_app();
    app.busy = true;
    app.termination_finished(
        app.snapshot.clone(),
        &TerminationReport {
            attempted: 2,
            delivered: vec![201],
            failures: vec![SignalFailure {
                pid: 204,
                reason: "process changed".into(),
            }],
        },
    );
    let notice = app.notice.unwrap();
    assert!(notice.error);
    assert!(notice.text.contains("1/2"));
    assert!(notice.text.contains("process changed"));
    assert!(!notice.text.contains("terminated"));
    assert!(!app.busy);
}

#[test]
fn mouse_termination_and_force_confirmation_are_visible_at_supported_widths() {
    for width in [80, 120, 140] {
        for (shortcut, button, mode) in [
            (
                "[x Terminate]",
                "[Enter Terminate]",
                TerminationMode::Terminate,
            ),
            (
                "[X Force kill]",
                "[Enter Force kill]",
                TerminationMode::ForceKill,
            ),
        ] {
            let mut app = fixture_app();
            for (label, confirm) in [(shortcut, false), (button, true)] {
                let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
                let mut hits = None;
                terminal
                    .draw(|frame| {
                        hits = Some(ui::draw_interactive(
                            frame,
                            &app,
                            Theme::Dark,
                            &mut ui::State::default(),
                        ))
                    })
                    .unwrap();
                let buffer = terminal.backend().buffer();
                let mut position = None;
                for y in 0..24 {
                    let line = (0..width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>();
                    if let Some(byte) = line.find(label) {
                        position = Some((line[..byte].chars().count() as u16, y));
                        break;
                    }
                }
                let (column, row) =
                    position.unwrap_or_else(|| panic!("missing {label} at width {width}"));
                let effect = app.handle_mouse(
                    hits.unwrap()
                        .action(MouseEvent {
                            kind: MouseEventKind::Down(MouseButton::Left),
                            column,
                            row,
                            modifiers: KeyModifiers::NONE,
                        })
                        .unwrap(),
                );
                if confirm {
                    assert_eq!(effect.terminate.unwrap().mode, mode);
                    assert!(app.busy);
                } else {
                    assert!(effect.terminate.is_none());
                    assert_eq!(proposed(&app).mode, mode);
                }
            }
        }
    }
}

#[test]
fn confirmation_lists_every_captured_process_path_instead_of_an_ellipsis() {
    let mut app = fixture_app();
    let mut target = app.snapshot.activity[0].identities[0].clone();
    for index in 0..8 {
        target.pid = 1000 + index;
        target.path = format!("/fixture/App.app/Contents/MacOS/helper-{index}");
        app.snapshot.activity[0].identities.push(target.clone());
    }
    key(&mut app, KeyCode::Char('X'));
    if let Some(Popup::Confirm { body, .. }) = &app.popup {
        for target in &proposed(&app).targets {
            assert!(body.contains(&format!("PID {} · {}", target.pid, target.path)));
        }
    } else {
        panic!("missing confirmation");
    }
    for (width, height) in [(50, 17), (80, 24), (120, 34)] {
        key(&mut app, KeyCode::End);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| ui::draw(frame, &app, Theme::Dark))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("helper-7"));
        assert!(text.contains("[Esc Cancel]"));
    }
    assert!(key(&mut app, KeyCode::Esc).terminate.is_none());
}
