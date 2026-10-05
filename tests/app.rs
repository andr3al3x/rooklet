mod common;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use rooklet::{
    app::{App, Popup, View, clean},
    model::*,
    ui::{self, Theme},
};
fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
#[test]
fn selection_tracks_process_identity_after_reordering() {
    let mut app = App::new(common::snapshot());
    app.handle(key(KeyCode::Down));
    let selected = app.selection[0].clone();
    let mut next = app.snapshot.clone();
    next.activity.reverse();
    app.update(next, false);
    assert_eq!(selected, app.selection[0]);
}
#[test]
fn incoming_permission_requires_explicit_confirmation() {
    let mut app = App::new(common::snapshot());
    app.view = View::Applications;
    app.handle(key(KeyCode::Down));
    assert!(app.handle(key(KeyCode::Char('b'))).mutation.is_none());
    assert!(matches!(app.popup, Some(Popup::Confirm { .. })));
    let mutation = app.handle(key(KeyCode::Enter)).mutation.unwrap();
    assert!(matches!(
        mutation,
        Mutation::Applications {
            action: Action::Block,
            ..
        }
    ));
    assert!(app.busy);
}
#[test]
fn cancelling_never_emits_a_mutation() {
    let mut app = App::new(common::snapshot());
    app.view = View::Applications;
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Char('b')));
    assert!(app.handle(key(KeyCode::Esc)).mutation.is_none());
    assert!(app.popup.is_none());
    assert!(!app.busy);
}
#[test]
fn frozen_activity_keeps_firewall_and_rules_current() {
    let mut app = App::new(common::snapshot());
    app.handle(key(KeyCode::Char(' ')));
    let original = app.snapshot.activity.len();
    let mut next = app.snapshot.clone();
    next.activity.clear();
    next.firewall.as_mut().unwrap().enabled = false;
    next.network.rules.clear();
    app.update(next, true);
    assert_eq!(app.snapshot.activity.len(), original);
    assert!(!app.snapshot.firewall.as_ref().unwrap().enabled);
    assert!(app.snapshot.network.rules.is_empty());
    app.handle(key(KeyCode::Char(' ')));
    assert!(app.snapshot.activity.is_empty());
    assert!(!app.snapshot.firewall.as_ref().unwrap().enabled);
}
#[test]
fn country_search_finds_connections_and_empty_search_recovers() {
    let mut app = App::new(common::snapshot());
    app.filters[0] = "no-such-country".into();
    app.handle(key(KeyCode::Down));
    assert_eq!(app.selected_index(), None);
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.selected_index(), Some(0));
    let ip = app
        .snapshot
        .activity
        .iter()
        .find_map(|p| p.connections.first())
        .unwrap()
        .remote_ip
        .clone();
    app.filters[0] = ip;
    assert!(
        app.activity_rows()
            .iter()
            .any(|row| matches!(row, rooklet::app::ActivityRow::Connection(_, _)))
    );
}
#[test]
fn network_editor_is_global_and_validates_before_confirmation() {
    let mut app = App::new(common::snapshot());
    app.view = View::Network;
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Char('n')));
    assert!(app.handle(key(KeyCode::Enter)).mutation.is_none());
    assert!(matches!(app.popup, Some(Popup::Network { .. })));
    if let Some(Popup::Network { draft, .. }) = &mut app.popup {
        draft.destination = "203.0.113.0/24".into();
    }
    app.handle(key(KeyCode::Enter));
    assert!(matches!(app.popup, Some(Popup::Confirm { .. })));
    let mutation = app.handle(key(KeyCode::Enter)).mutation.unwrap();
    assert!(matches!(mutation, Mutation::NetworkRules(_)));
}
#[test]
fn unfreeze_after_failed_poll_never_restores_firewall_health() {
    let mut app = App::new(common::snapshot());
    app.handle(key(KeyCode::Char(' ')));
    app.observation_failed("statistics disconnected".into());
    app.handle(key(KeyCode::Char(' ')));
    assert!(app.snapshot.firewall.is_none());
}
#[test]
fn all_views_dialogs_and_themes_render_at_supported_sizes() {
    for (width, height) in [(140, 40), (100, 30), (80, 24), (50, 17), (20, 5), (1, 1)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut app = App::new(common::snapshot());
        for theme in [Theme::Dark, Theme::Light, Theme::Mono] {
            for view in View::ALL {
                app.view = view;
                terminal.draw(|frame| ui::draw(frame, &app, theme)).unwrap();
                app.popup = Some(Popup::Help);
                terminal.draw(|frame| ui::draw(frame, &app, theme)).unwrap();
                app.popup = None;
            }
            app.view = View::Network;
            app.handle(key(KeyCode::Char('n')));
            terminal.draw(|frame| ui::draw(frame, &app, theme)).unwrap();
            app.popup = None;
        }
    }
}
#[test]
fn render_never_claims_outgoing_app_enforcement_or_observed_verdicts() {
    let mut terminal = Terminal::new(TestBackend::new(120, 34)).unwrap();
    let app = App::new(common::snapshot());
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
    assert!(text.contains("Country"));
    assert!(text.contains("↓/s"));
    assert!(!text.contains("DECISION"));
    assert!(!text.contains("Requests"));
}
#[test]
fn untrusted_labels_cannot_inject_terminal_or_bidi_controls() {
    assert_eq!(clean("hello\x1b[31m\r\n\u{202e}app"), "hello[31mapp");
}

#[test]
fn grouped_app_selection_and_expansion_survive_helper_pid_changes() {
    let mut app = App::new(common::snapshot());
    app.handle(key(KeyCode::Enter));
    let key = app.selection[0].clone();
    let expanded = app.expanded.clone();
    let mut next = app.snapshot.clone();
    next.activity[0].pid += 1000;
    app.update(next, false);
    assert_eq!(app.selection[0], key);
    assert_eq!(app.expanded, expanded);
    assert!(
        app.activity_rows()
            .iter()
            .any(|row| matches!(row, rooklet::app::ActivityRow::Connection(_, _)))
    );
}

#[test]
fn expansion_history_is_pruned_during_process_churn() {
    let mut app = App::new(common::snapshot());
    for generation in 0..100 {
        app.handle(key(KeyCode::Enter));
        assert_eq!(app.expanded.len(), 1);
        let mut next = common::snapshot();
        next.activity.truncate(1);
        next.activity[0].path = Some(format!("/fixture/Generation-{generation}.app"));
        app.update(next, false);
        assert!(app.expanded.is_empty());
    }
}

#[test]
fn standalone_expansion_does_not_survive_pid_reuse() {
    let mut snapshot = common::snapshot();
    snapshot.activity = vec![snapshot.activity[2].clone()];
    let process = &mut snapshot.activity[0];
    assert!(!process.path.as_ref().unwrap().ends_with(".app"));
    let mut identity = process
        .identities
        .first()
        .cloned()
        .unwrap_or_else(|| common::snapshot().activity[0].identities[0].clone());
    identity.pid = process.pid;
    identity.path = process.path.clone().unwrap();
    identity.bundle_path = None;
    process.identities = vec![identity];
    let mut app = App::new(snapshot);
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.expanded.len(), 1);
    let mut next = app.snapshot.clone();
    next.activity[0].identities[0].pid_version += 1;
    next.activity[0].identities[0].start_sec += 1;
    app.update(next, false);
    assert!(app.expanded.is_empty());
}

#[test]
fn uncaptured_process_expansion_is_limited_to_the_current_observation() {
    let mut snapshot = common::snapshot();
    snapshot.activity = vec![snapshot.activity[2].clone()];
    snapshot.activity[0].identities.clear();
    let mut app = App::new(snapshot);
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.expanded.len(), 1);
    app.update(app.snapshot.clone(), false);
    assert!(app.expanded.is_empty());
}

#[test]
fn filtered_network_rows_keep_saved_first_match_positions() {
    let mut snapshot = common::snapshot();
    let mut second = snapshot.network.rules[0].clone();
    second.id = "second-rule".into();
    second.name = "Second target".into();
    snapshot.network.rules.push(second);
    let mut app = App::new(snapshot);
    app.view = View::Network;
    app.filters[2] = "Second target".into();
    for (width, height) in [(80, 24), (120, 34)] {
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
        assert!(text.contains("02 on"));
        assert!(!text.contains("01 on"));
    }
    assert_eq!(app.rule_rows()[0].0, 1);
}
#[test]
fn editing_a_disabled_rule_preserves_its_disabled_state() {
    let mut snapshot = common::snapshot();
    snapshot.network.rules[0].enabled = false;
    let mut app = App::new(snapshot);
    app.handle(key(KeyCode::Char('3')));
    app.handle(key(KeyCode::Enter));
    app.handle(key(KeyCode::Enter));
    let effect = app.handle(key(KeyCode::Enter));
    let Mutation::NetworkRules(rules) = effect.mutation.unwrap() else {
        panic!("wrong mutation")
    };
    assert!(!rules[0].enabled);
}
#[test]
fn search_text_never_triggers_authentication() {
    let mut app = App::new(common::snapshot());
    app.handle(key(KeyCode::Char('/')));
    assert!(!app.handle(key(KeyCode::Char('u'))).authenticate);
    assert_eq!(app.filter(), "u");
    app.handle(key(KeyCode::Enter));
    assert!(app.handle(key(KeyCode::Char('u'))).authenticate);
}

#[test]
fn country_update_is_explicit_unprivileged_and_serialized() {
    let mut app = App::new(common::snapshot());
    assert!(!app.handle(key(KeyCode::Char('g'))).update_geoip);
    app.handle(key(KeyCode::Char('4')));
    let original = app.snapshot.firewall.clone();
    let effect = app.handle(key(KeyCode::Char('g')));
    assert!(effect.update_geoip);
    assert!(effect.mutation.is_none());
    assert!(!effect.authenticate);
    assert!(app.busy);
    assert!(app.popup.is_none());
    assert!(!app.handle(key(KeyCode::Char('g'))).update_geoip);
    assert!(!app.handle(key(KeyCode::Char('u'))).authenticate);
    app.handle(key(KeyCode::Enter));
    assert!(app.popup.is_none());
    app.operation_failed("Country download failed".into());
    assert!(!app.busy);
    assert_eq!(app.snapshot.firewall, original);
    assert!(app.notice.as_ref().unwrap().error);
    app.handle(key(KeyCode::Char('g')));
    app.geoip_updated(common::snapshot());
    assert!(!app.busy);
    assert_eq!(app.snapshot.firewall, original);
    assert!(app.notice.as_ref().unwrap().text.contains("updated"));
    assert!(!app.notice.as_ref().unwrap().error);
}

#[test]
fn search_and_dialog_keys_cannot_start_a_country_download() {
    let mut app = App::new(common::snapshot());
    app.handle(key(KeyCode::Char('4')));
    app.handle(key(KeyCode::Char('/')));
    assert!(!app.handle(key(KeyCode::Char('g'))).update_geoip);
    assert_eq!(app.filter(), "g");
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::Char('?')));
    assert!(!app.handle(key(KeyCode::Char('g'))).update_geoip);
    assert!(!app.busy);
    assert!(matches!(app.popup, Some(Popup::Help)));
}

#[test]
fn settings_show_country_provider_age_attribution_and_action_at_eighty_columns() {
    let mut app = App::new(common::snapshot());
    app.view = View::Settings;
    // Explicit rendering fixture; no live backend or network requests.
    app.snapshot.geoip = Some("DB-IP Lite · 3 days old · CC BY 4.0".into());
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
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
    assert!(text.contains("DB-IP Lite · 3 days old"));
    assert!(text.contains("db-ip.com · CC BY 4.0"));
    assert!(text.contains("[g Update countries]"));
}
