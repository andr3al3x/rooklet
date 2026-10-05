mod common;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use rooklet::{
    app::{App, Effect, MouseAction, Popup, SETTINGS, View},
    model::{Action, Application, Mutation, Protocol},
    ui::{self, HitMap, Theme},
};

fn fixture_app() -> App {
    App::new(common::snapshot())
}

fn render(app: &App, width: u16, height: u16) -> (HitMap, Buffer) {
    render_with_state(app, width, height, &mut ui::State::default())
}

fn render_with_state(
    app: &App,
    width: u16,
    height: u16,
    state: &mut ui::State,
) -> (HitMap, Buffer) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let mut hits = None;
    terminal
        .draw(|frame| hits = Some(ui::draw_interactive(frame, app, Theme::Dark, state)))
        .unwrap();
    (hits.unwrap(), terminal.backend().buffer().clone())
}

fn point(buffer: &Buffer, text: &str) -> (u16, u16) {
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let suffix = (x..buffer.area.width)
                .map(|column| buffer[(column, y)].symbol())
                .collect::<String>();
            if suffix.starts_with(text) {
                return (x, y);
            }
        }
    }
    panic!("text not rendered: {text}")
}

fn event(kind: MouseEventKind, (column, row): (u16, u16)) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn click(app: &mut App, text: &str) -> Effect {
    let (hits, buffer) = render(app, 80, 24);
    let action = hits
        .action(event(
            MouseEventKind::Down(MouseButton::Left),
            point(&buffer, text),
        ))
        .unwrap();
    app.handle_mouse(action)
}

fn key(app: &mut App, code: KeyCode) -> Effect {
    app.handle(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn tabs_and_footer_shortcuts_work_while_searching() {
    let mut app = fixture_app();
    for view in View::ALL {
        assert!(click(&mut app, view.title()).mutation.is_none());
        assert_eq!(app.view, view);
    }
    click(&mut app, "[/ Search]");
    assert!(app.searching);
    assert!(click(&mut app, "[u Unlock]").authenticate);
    assert!(!app.searching);
    assert_eq!(app.filter(), "");
    click(&mut app, "[? Help]");
    assert!(matches!(app.popup, Some(Popup::Help)));
    click(&mut app, "[Esc Close]");
    assert!(app.popup.is_none());
    assert!(click(&mut app, "[q Quit]").quit);
}

#[test]
fn activity_row_click_selects_double_click_expands_and_gutter_collapses() {
    let mut app = fixture_app();
    let name = app.snapshot.activity[0].name.clone();
    let identity = app.activity_rows()[0].key();
    click(&mut app, &name);
    assert_eq!(app.selected_key(), Some(identity.as_str()));
    assert!(app.expanded.is_empty());
    click(&mut app, &name);
    assert!(app.expanded.contains(&identity));
    let peer = app.snapshot.activity[0].connections[0].remote_ip.clone();
    click(&mut app, &peer);
    click(&mut app, &peer);
    assert!(matches!(app.popup, Some(Popup::Inspect(_))));
    click(&mut app, "[Esc Close]");
    let (hits, buffer) = render(&app, 80, 24);
    let (_, row) = point(&buffer, &name);
    let action = hits
        .action(event(MouseEventKind::Down(MouseButton::Left), (3, row)))
        .unwrap();
    assert!(app.handle_mouse(action).mutation.is_none());
    assert!(!app.expanded.contains(&identity));
}

#[test]
fn double_click_settings_only_reviews_and_modal_blocks_background() {
    for (setting, label) in SETTINGS {
        let mut app = fixture_app();
        click(&mut app, "Settings");
        let original = app.snapshot.firewall.clone();
        assert!(click(&mut app, label).mutation.is_none());
        assert!(app.popup.is_none());
        assert!(click(&mut app, label).mutation.is_none());
        assert!(matches!(app.popup, Some(Popup::Confirm { .. })));
        assert_eq!(app.snapshot.firewall, original);
        let (hits, buffer) = render(&app, 80, 24);
        for position in [
            point(&buffer, "Activity"),
            point(&buffer, "[q Quit]"),
            (0, 0),
        ] {
            assert!(
                hits.action(event(MouseEventKind::Down(MouseButton::Left), position))
                    .is_none()
            );
            assert!(
                hits.action(event(MouseEventKind::ScrollDown, position))
                    .is_none()
            );
        }
        assert!(click(&mut app, "[Esc Cancel]").mutation.is_none());
        assert!(app.popup.is_none());
        assert!(!app.busy);
        click(&mut app, "[Enter Toggle]");
        let effect = click(&mut app, "[Enter Apply]");
        assert!(
            matches!(effect.mutation, Some(Mutation::Setting(actual, value)) if actual == setting && value != original.unwrap().get(setting))
        );
        assert!(app.busy);
    }
}

#[test]
fn application_double_click_requires_confirmation_and_respects_busy_state() {
    for blocked in [false, true] {
        let mut app = fixture_app();
        app.snapshot.applications[0].blocked = blocked;
        let name = app.snapshot.applications[0].name.clone();
        let path = app.snapshot.applications[0].path.clone();
        click(&mut app, "Applications");
        click(&mut app, &name);
        assert!(app.popup.is_none());
        assert!(click(&mut app, &name).mutation.is_none());
        let effect = click(&mut app, "[Enter Apply]");
        assert!(
            matches!(effect.mutation, Some(Mutation::Applications { paths: actual, action }) if actual == vec![path] && action == if blocked { Action::Allow } else { Action::Block })
        );
        click(&mut app, &name);
        assert!(click(&mut app, &name).mutation.is_none());
        assert!(app.popup.is_none());
    }
}

fn many_apps() -> App {
    let mut app = fixture_app();
    app.snapshot.applications = (0..60)
        .map(|index| Application {
            path: format!("/Applications/Program{index:02}.app"),
            name: format!("Program{index:02}"),
            blocked: false,
        })
        .collect();
    app.handle_mouse(MouseAction::View(View::Applications));
    app
}

#[test]
fn visible_row_targets_follow_table_scroll_offset_filter_and_reordering() {
    let mut app = many_apps();
    app.selection[1] = Some("/Applications/Program47.app".into());
    for (width, height) in [(80, 24), (140, 40)] {
        let (hits, buffer) = render(&app, width, height);
        let mut seen = Vec::new();
        for row in 0..height {
            let text = (0..width)
                .map(|x| buffer[(x, row)].symbol())
                .collect::<String>();
            for application in app.applications() {
                if text.contains(&application.name) {
                    let (column, _) = point(&buffer, &application.name);
                    assert_eq!(
                        hits.action(event(
                            MouseEventKind::Down(MouseButton::Left),
                            (column, row)
                        )),
                        Some(MouseAction::Row {
                            view: View::Applications,
                            key: application.path.clone(),
                            activate: false
                        })
                    );
                    seen.push(application.name.clone());
                }
            }
        }
        assert!(!seen.is_empty());
        assert!(!seen.contains(&"Program00".into()));
        assert!(seen.contains(&"Program47".into()));
    }
    let selected = app.selection[1].clone();
    let mut next = app.snapshot.clone();
    next.applications.reverse();
    app.update(next, false);
    assert_eq!(app.selection[1], selected);
    app.filters[1] = "Program4".into();
    click(&mut app, "Program44");
    assert_eq!(app.selected_key(), Some("/Applications/Program44.app"));
}

#[test]
fn scrolled_rows_stay_under_the_pointer_between_clicks_and_redraws() {
    let mut app = many_apps();
    let mut state = ui::State::default();
    app.selection[1] = Some("/Applications/Program47.app".into());
    let (hits, buffer) = render_with_state(&app, 80, 24, &mut state);
    // Click the top visible row, away from the selected bottom row.
    let (position, action) = (0..24)
        .find_map(|row| {
            let position = (7, row);
            match hits.action(event(MouseEventKind::Down(MouseButton::Left), position)) {
                Some(action @ MouseAction::Row { .. }) => Some((position, action)),
                _ => None,
            }
        })
        .unwrap();
    let MouseAction::Row { key: identity, .. } = &action else {
        unreachable!()
    };
    assert_ne!(identity, "/Applications/Program47.app");
    let name = app
        .snapshot
        .applications
        .iter()
        .find(|application| application.path == *identity)
        .unwrap()
        .name
        .clone();
    assert_eq!(point(&buffer, &name).1, position.1);
    app.handle_mouse(action.clone());
    let (next_hits, next_buffer) = render_with_state(&app, 80, 24, &mut state);
    assert_eq!(point(&next_buffer, &name).1, position.1);
    let next_action = next_hits
        .action(event(MouseEventKind::Down(MouseButton::Left), position))
        .unwrap();
    assert_eq!(action, next_action);
    assert!(app.handle_mouse(next_action).mutation.is_none());
    assert!(matches!(app.popup, Some(Popup::Confirm { .. })));
}

#[test]
fn wheel_moves_selection_only_over_list_body() {
    let mut app = many_apps();
    let (hits, buffer) = render(&app, 80, 24);
    let position = point(&buffer, "Program00");
    for _ in 0..20 {
        let action = hits
            .action(event(MouseEventKind::ScrollDown, position))
            .unwrap();
        assert!(app.handle_mouse(action).mutation.is_none());
    }
    assert_eq!(app.selected_index(), Some(59));
    let action = hits
        .action(event(MouseEventKind::ScrollUp, position))
        .unwrap();
    app.handle_mouse(action);
    assert_eq!(app.selected_index(), Some(56));
    assert!(
        hits.action(event(
            MouseEventKind::ScrollDown,
            point(&buffer, "APPLICATION")
        ))
        .is_none()
    );
    assert!(
        hits.action(event(
            MouseEventKind::Down(MouseButton::Left),
            (0, position.1)
        ))
        .is_none()
    );
}

#[test]
fn stale_row_actions_cannot_select_another_app_or_open_a_dialog() {
    let mut app = many_apps();
    let (hits, buffer) = render(&app, 80, 24);
    let action = hits
        .action(event(
            MouseEventKind::Down(MouseButton::Left),
            point(&buffer, "Program00"),
        ))
        .unwrap();
    let mut next = app.snapshot.clone();
    next.applications.remove(0);
    app.update(next, false);
    let selected = app.selection[1].clone();
    for _ in 0..2 {
        assert!(app.handle_mouse(action.clone()).mutation.is_none());
    }
    assert_eq!(app.selection[1], selected);
    assert!(app.popup.is_none());
    app.filters[1] = "Program5".into();
    assert!(app.handle_mouse(action).mutation.is_none());
    assert!(app.popup.is_none());
}

#[test]
fn editor_mouse_focus_and_choices_remain_aligned_with_long_text() {
    let mut app = fixture_app();
    click(&mut app, "Network");
    click(&mut app, "[n Add]");
    if let Some(Popup::Network { draft, .. }) = &mut app.popup {
        draft.name = "x".repeat(200);
    }
    click(&mut app, "Protocol");
    assert!(
        matches!(&app.popup, Some(Popup::Network { draft, field: 2 }) if draft.protocol == Protocol::Tcp)
    );
    click(&mut app, "Remote IP/CIDR");
    for c in "203.0.113.0/24".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    click(&mut app, "Destination port");
    for c in "443".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    assert!(
        matches!(&app.popup, Some(Popup::Network { draft, field: 1 }) if draft.destination == "203.0.113.0/24" && draft.port == "443")
    );
    if let Some(Popup::Network { draft, .. }) = &mut app.popup {
        draft.name = "Test rule".into();
    }
    assert!(click(&mut app, "[Enter Review]").mutation.is_none());
    assert!(matches!(app.popup, Some(Popup::Confirm { .. })));
    assert!(click(&mut app, "[Esc Cancel]").mutation.is_none());
    assert!(app.popup.is_none());
}

#[test]
fn ignored_mouse_events_and_tiny_windows_have_no_targets() {
    let app = fixture_app();
    let (hits, buffer) = render(&app, 80, 24);
    let position = point(&buffer, "Settings");
    for kind in [
        MouseEventKind::Up(MouseButton::Left),
        MouseEventKind::Down(MouseButton::Right),
        MouseEventKind::Drag(MouseButton::Left),
        MouseEventKind::Moved,
        MouseEventKind::ScrollLeft,
        MouseEventKind::ScrollRight,
    ] {
        assert!(hits.action(event(kind, position)).is_none());
    }
    let mut modified = event(MouseEventKind::Down(MouseButton::Left), position);
    modified.modifiers = KeyModifiers::SHIFT;
    assert!(hits.action(modified).is_none());
    for (width, height) in [(140, 40), (80, 24), (50, 17), (20, 5), (1, 1)] {
        let (hits, _) = render(&app, width, height);
        assert!(
            hits.action(event(
                MouseEventKind::Down(MouseButton::Left),
                (width, height)
            ))
            .is_none()
        );
        if width < 50 {
            for row in 0..height {
                for column in 0..width {
                    assert!(
                        hits.action(event(
                            MouseEventKind::Down(MouseButton::Left),
                            (column, row)
                        ))
                        .is_none()
                    );
                }
            }
        }
    }
}

#[test]
fn country_update_button_uses_the_same_unprivileged_action_and_is_modal_safe() {
    let mut app = fixture_app();
    click(&mut app, "Settings");
    let (hits, buffer) = render(&app, 80, 24);
    let position = point(&buffer, "[g Update countries]");
    click(&mut app, "[? Help]");
    let (modal, _) = render(&app, 80, 24);
    assert!(
        modal
            .action(event(MouseEventKind::Down(MouseButton::Left), position))
            .is_none()
    );
    click(&mut app, "[Esc Close]");
    let effect = app.handle_mouse(
        hits.action(event(MouseEventKind::Down(MouseButton::Left), position))
            .unwrap(),
    );
    assert!(effect.update_geoip);
    assert!(effect.mutation.is_none());
    assert!(!effect.authenticate);
    assert!(app.busy);
    assert!(!click(&mut app, "[g Update countries]").update_geoip);
}

#[test]
fn confirmation_wheel_uses_visible_content_geometry_and_never_moves_background() {
    let mut app = fixture_app();
    key(&mut app, KeyCode::Char('b'));
    if let Some(Popup::Confirm { body, .. }) = &mut app.popup {
        *body = (0..50)
            .map(|index| format!("Registered target {index:02}"))
            .collect::<Vec<_>>()
            .join("\n");
    }
    let selected = app.selection.clone();
    for (width, height) in [(50, 17), (80, 24), (120, 34)] {
        key(&mut app, KeyCode::Home);
        let (hits, buffer) = render(&app, width, height);
        let position = point(&buffer, "Registered target 00");
        assert!(
            hits.action(event(MouseEventKind::Down(MouseButton::Left), position))
                .is_none()
        );
        let action = hits
            .action(event(MouseEventKind::ScrollDown, position))
            .unwrap();
        assert_eq!(action, MouseAction::DialogScroll(3));
        assert!(app.handle_mouse(action).mutation.is_none());
        assert_eq!(app.selection, selected);
        let (_, buffer) = render(&app, width, height);
        point(&buffer, "Registered target 03");
        assert!(
            hits.action(event(MouseEventKind::ScrollDown, (0, 0)))
                .is_none()
        );
    }
    assert!(key(&mut app, KeyCode::Esc).mutation.is_none());
}
