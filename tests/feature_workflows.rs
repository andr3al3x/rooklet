mod common;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use xield::{
    app::{ActivitySort, App, MouseAction, Popup, View},
    model::{Action, Direction, Mutation, NetworkRule, Protocol},
    ui::{self, HitMap, Theme},
};

fn fixture_app() -> App {
    App::new(common::snapshot())
}
fn key(app: &mut App, code: KeyCode) -> xield::app::Effect {
    app.handle(KeyEvent::new(code, KeyModifiers::NONE))
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
fn text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn click(app: &mut App, label: &str, width: u16, height: u16) -> xield::app::Effect {
    let (hits, buffer) = render(app, width, height);
    for y in 0..height {
        for x in 0..width {
            if (x..width)
                .map(|column| buffer[(column, y)].symbol())
                .collect::<String>()
                .starts_with(label)
            {
                let action = hits
                    .action(MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: x,
                        row: y,
                        modifiers: KeyModifiers::NONE,
                    })
                    .unwrap();
                return app.handle_mouse(action);
            }
        }
    }
    panic!("missing {label} in {}", text(&buffer));
}
fn rule(id: &str, destination: &str, port: Option<u16>) -> NetworkRule {
    NetworkRule {
        id: id.into(),
        name: id.into(),
        destination: destination.into(),
        port,
        action: Action::Allow,
        protocol: Protocol::Tcp,
        direction: Direction::Outbound,
        interface: None,
        enabled: true,
    }
}

#[test]
fn peer_draft_captures_metadata_but_only_applies_after_two_review_steps() {
    for protocol in [Protocol::Tcp, Protocol::Udp, Protocol::Any] {
        let mut app = fixture_app();
        let flow = &mut app.snapshot.activity[0].connections[0];
        flow.protocol = protocol;
        flow.remote_port = Some(5353);
        let remote = flow.remote_ip.clone();
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Down);
        assert!(key(&mut app, KeyCode::Char('n')).mutation.is_none());
        let Some(Popup::Network { draft, .. }) = &app.popup else {
            panic!("missing draft")
        };
        assert_eq!(draft.destination, remote);
        assert_eq!(draft.protocol, protocol);
        assert_eq!(draft.direction, Direction::Outbound);
        assert_eq!(draft.action, Action::Block);
        assert_eq!(
            draft.port,
            if protocol == Protocol::Any {
                ""
            } else {
                "5353"
            }
        );
        assert!(key(&mut app, KeyCode::Enter).mutation.is_none());
        let Some(Popup::Confirm { body, .. }) = &app.popup else {
            panic!("missing review")
        };
        assert!(body.contains("ALL applications"));
        assert!(body.contains(&remote));
        assert!(body.contains("Existing connections"));
        let Some(Mutation::NetworkRules(rules)) = key(&mut app, KeyCode::Enter).mutation else {
            panic!("missing mutation")
        };
        assert!(rules.iter().any(|rule| rule.destination == remote
            && rule.protocol == protocol
            && rule.direction == Direction::Outbound));
    }
}

#[test]
fn peer_shortcut_requires_a_selected_peer_and_available_pf_configuration() {
    let mut app = fixture_app();
    assert!(key(&mut app, KeyCode::Char('n')).mutation.is_none());
    assert!(app.popup.is_none());
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Down);
    for unavailable in [true, false] {
        app.snapshot.network.rules_available = !unavailable;
        app.snapshot.network.configured = unavailable;
        assert!(key(&mut app, KeyCode::Char('n')).mutation.is_none());
        assert!(app.popup.is_none());
    }
}

#[test]
fn mouse_peer_rule_and_cancel_follow_the_keyboard_review_flow() {
    let mut app = fixture_app();
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Down);
    assert!(click(&mut app, "[n IP rule]", 80, 24).mutation.is_none());
    assert!(matches!(app.popup, Some(Popup::Network { .. })));
    assert!(click(&mut app, "[Enter Review]", 80, 24).mutation.is_none());
    assert!(matches!(app.popup, Some(Popup::Confirm { .. })));
    assert!(click(&mut app, "[Esc Cancel]", 80, 24).mutation.is_none());
    assert!(!app.busy && app.popup.is_none());
}

#[test]
fn sorting_is_available_with_a_filter_and_stays_inside_modal_boundaries() {
    let mut app = fixture_app();
    app.filters[0] = "proto:tcp".into();
    let selected = app.selection[0].clone();
    app.searching = true;
    assert!(click(&mut app, "Sort:", 80, 24).mutation.is_none());
    assert_eq!(app.activity_sort, ActivitySort::DownloadRate);
    assert_eq!(app.filters[0], "proto:tcp");
    assert_eq!(app.selection[0], selected);
    assert!(!app.searching);
    key(&mut app, KeyCode::Char('?'));
    let (hits, buffer) = render(&app, 140, 40);
    for y in 0..40 {
        for x in 0..140 {
            if (x..140)
                .map(|column| buffer[(column, y)].symbol())
                .collect::<String>()
                .starts_with("Sort:")
            {
                assert!(
                    hits.action(MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: x,
                        row: y,
                        modifiers: KeyModifiers::NONE
                    })
                    .is_none()
                );
            }
        }
    }
    assert!(key(&mut app, KeyCode::Char('s')).mutation.is_none());
    assert_eq!(app.activity_sort, ActivitySort::DownloadRate);
}

#[test]
fn shadow_warnings_are_present_before_edit_and_reorder_confirmation() {
    let mut app = fixture_app();
    app.snapshot.network.rules = vec![
        rule("broad", "any", None),
        rule("narrow", "203.0.113.0/24", Some(443)),
    ];
    app.view = View::Network;
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    assert!(key(&mut app, KeyCode::Enter).mutation.is_none());
    let Some(Popup::Confirm { body, .. }) = &app.popup else {
        panic!("missing review")
    };
    assert!(body.contains("narrow is fully shadowed by #1 broad"));
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('+'));
    let Some(Popup::Confirm { body, .. }) = &app.popup else {
        panic!("missing reorder review")
    };
    assert!(!body.contains("fully shadowed"));
}

#[test]
fn explanation_fields_mouse_geometry_and_read_only_result_work_at_supported_sizes() {
    for (width, height) in [(140, 40), (80, 24), (50, 17)] {
        let mut app = fixture_app();
        app.snapshot.network.rules = vec![
            rule("web", "203.0.113.0/24", Some(443)),
            rule("fallback", "any", None),
        ];
        app.view = View::Network;
        key(&mut app, KeyCode::Char('w'));
        assert!(matches!(app.popup, Some(Popup::Explain { .. })));
        click(&mut app, "Remote IP", width, height);
        for ch in "203.0.113.5".chars() {
            key(&mut app, KeyCode::Char(ch));
        }
        let (_, buffer) = render(&app, width, height);
        assert!(text(&buffer).contains("Undetermined"));
        click(&mut app, "Destination port", width, height);
        for ch in "443".chars() {
            key(&mut app, KeyCode::Char(ch));
        }
        let (_, buffer) = render(&app, width, height);
        assert!(text(&buffer).contains("First match: #1 web"));
        click(&mut app, "Protocol", width, height);
        assert!(
            matches!(&app.popup, Some(Popup::Explain { draft, field: 2 }) if draft.protocol == Protocol::Udp)
        );
        click(&mut app, "Direction", width, height);
        assert!(
            matches!(&app.popup, Some(Popup::Explain { draft, field: 3 }) if draft.direction == Direction::Inbound)
        );
        assert!(
            click(&mut app, "[Esc Close]", width, height)
                .mutation
                .is_none()
        );
        assert!(!app.busy && app.popup.is_none());
    }
}

#[test]
fn invalid_filters_are_visible_and_cannot_open_peer_rule_dialogs() {
    let mut app = fixture_app();
    app.filters[0] = "proto:typo".into();
    let (_, buffer) = render(&app, 80, 24);
    assert!(text(&buffer).contains("Invalid proto:"));
    assert!(key(&mut app, KeyCode::Char('n')).mutation.is_none());
    assert!(app.popup.is_none());
    assert!(
        app.handle_mouse(MouseAction::DialogField(2))
            .mutation
            .is_none()
    );
}

#[test]
fn text_editors_bound_complete_utf8_characters_and_reject_controls() {
    let mut app = fixture_app();
    app.popup = Some(Popup::Application {
        path: "a".repeat(4095),
    });
    key(&mut app, KeyCode::Char('界'));
    assert!(matches!(&app.popup, Some(Popup::Application { path }) if path.len() == 4095));
    key(&mut app, KeyCode::Char('a'));
    assert!(matches!(&app.popup, Some(Popup::Application { path }) if path.len() == 4096));
    key(&mut app, KeyCode::Esc);
    app.view = View::Network;
    key(&mut app, KeyCode::Char('n'));
    if let Some(Popup::Network { draft, field }) = &mut app.popup {
        draft.name = "a".repeat(255);
        *field = 6;
    }
    key(&mut app, KeyCode::Char('é'));
    key(&mut app, KeyCode::Char('\u{1b}'));
    assert!(matches!(&app.popup, Some(Popup::Network { draft, .. }) if draft.name.len() == 255));
    key(&mut app, KeyCode::Char('a'));
    assert!(matches!(&app.popup, Some(Popup::Network { draft, .. }) if draft.name.len() == 256));
    key(&mut app, KeyCode::Backspace);
    assert!(matches!(&app.popup, Some(Popup::Network { draft, .. }) if draft.name.len() == 255));
    key(&mut app, KeyCode::Esc);
    app.view = View::Activity;
    app.filters[0] = "a".repeat(255);
    key(&mut app, KeyCode::Char('/'));
    key(&mut app, KeyCode::Char('界'));
    assert_eq!(app.filters[0].len(), 255);
    key(&mut app, KeyCode::Char('a'));
    key(&mut app, KeyCode::Char('\u{1b}'));
    assert_eq!(app.filters[0].len(), 256);
}
