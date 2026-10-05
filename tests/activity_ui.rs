mod common;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use rooklet::{
    app::{App, Popup, process_key},
    ui::{self, Theme},
};
use rooklet_core::model::Application;

fn fixture_app() -> App {
    let mut snapshot = common::snapshot();
    snapshot.notices.clear();
    for process in &mut snapshot.activity {
        process.rate_in = 101;
        process.rate_out = 202;
        process.bytes_in = 303;
        process.bytes_out = 404;
        for peer in &mut process.connections {
            peer.bytes_in = 505;
            peer.bytes_out = 606;
        }
    }
    App::new(snapshot)
}

fn render(app: &App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| ui::draw(frame, app, Theme::Dark))
        .unwrap();
    terminal.backend().buffer().clone()
}

fn lines(buffer: &Buffer) -> Vec<String> {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        })
        .collect()
}

fn row(buffer: &Buffer, label: &str) -> u16 {
    lines(buffer)
        .iter()
        .position(|line| line.contains(label))
        .unwrap_or_else(|| panic!("missing {label:?} in\n{}", lines(buffer).join("\n"))) as u16
}

fn header_column(buffer: &Buffer, label: &str) -> u16 {
    let y = row(buffer, "APP / PEER");
    (0..buffer.area.width)
        .find(|&x| {
            (x..buffer.area.width)
                .map(|column| buffer[(column, y)].symbol())
                .collect::<String>()
                .starts_with(label)
        })
        .unwrap()
}

fn cell(buffer: &Buffer, y: u16, header: &str, next: Option<&str>) -> String {
    let start = header_column(buffer, header);
    let end = next
        .map(|label| header_column(buffer, label))
        .unwrap_or(buffer.area.width - 3);
    (start..end)
        .map(|x| buffer[(x, y)].symbol())
        .collect::<String>()
        .trim()
        .to_string()
}

fn compact(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn key(app: &mut App, code: KeyCode) {
    app.handle(KeyEvent::new(code, KeyModifiers::NONE));
}

#[test]
fn all_process_rows_show_peer_counts_and_session_totals_without_an_inspector() {
    let mut app = fixture_app();
    let extra_peer = app.snapshot.activity[1].connections[0].clone();
    app.snapshot.activity[1].connections.push(extra_peer);
    for (width, height) in [(80, 24), (120, 34), (140, 40)] {
        let buffer = render(&app, width, height);
        assert!(!lines(&buffer).join("\n").contains("INSPECTOR"));
        let unselected = row(&buffer, "Terminal");
        assert_eq!(cell(&buffer, unselected, "Peers", Some("Country")), "2");
        assert_eq!(
            compact(&cell(&buffer, unselected, "↓ Total", Some("↑ Total"))),
            "303B"
        );
        let next = (width >= 120).then_some("Incoming");
        assert_eq!(compact(&cell(&buffer, unselected, "↑ Total", next)), "404B");
    }
}

#[test]
fn expanded_peers_have_totals_but_never_inherit_process_rates() {
    let mut app = fixture_app();
    let identity = process_key(&app.snapshot.activity[0]);
    app.expanded.insert(identity);
    for (width, height) in [(80, 24), (120, 34), (140, 40)] {
        let buffer = render(&app, width, height);
        let process = row(&buffer, "Safari");
        let peer = row(&buffer, "151.101.1.69");
        assert_eq!(
            compact(&cell(&buffer, process, "↓/s", Some("↑/s"))),
            "101B/s"
        );
        assert_eq!(
            compact(&cell(&buffer, process, "↑/s", Some("↓ Total"))),
            "202B/s"
        );
        assert_eq!(cell(&buffer, peer, "↓/s", Some("↑/s")), "");
        assert_eq!(cell(&buffer, peer, "↑/s", Some("↓ Total")), "");
        assert_eq!(
            compact(&cell(&buffer, peer, "↓ Total", Some("↑ Total"))),
            "505B"
        );
        let next = (width >= 120).then_some("Incoming");
        assert_eq!(compact(&cell(&buffer, peer, "↑ Total", next)), "606B");
        let peer_line = &lines(&buffer)[usize::from(peer)];
        assert!(peer_line.contains("443"));
        assert!(peer_line.contains("tcp"));
        assert!(peer_line.contains("US"));
    }
}

#[test]
fn incoming_entries_match_paths_and_distinguish_unavailable_and_unlisted() {
    let mut app = fixture_app();
    app.resources_visible = false;
    let terminal_path = app.snapshot.activity[1].path.clone().unwrap();
    app.snapshot.applications.push(Application {
        path: terminal_path.clone(),
        name: "Different display name".into(),
        blocked: true,
    });
    app.snapshot.applications.push(Application {
        path: "/different/mDNSResponder".into(),
        name: "mDNSResponder".into(),
        blocked: false,
    });
    for (width, height) in [(120, 34), (140, 40)] {
        let buffer = render(&app, width, height);
        for (name, status) in [
            ("Safari", "Allow"),
            ("Terminal", "Block"),
            ("mDNSResponder", "Unlisted"),
        ] {
            assert_eq!(
                cell(&buffer, row(&buffer, name), "Incoming", Some("Path")),
                status
            );
        }
        app.snapshot.applications_available = false;
        let buffer = render(&app, width, height);
        for name in ["Safari", "Terminal", "mDNSResponder"] {
            assert_eq!(
                cell(&buffer, row(&buffer, name), "Incoming", Some("Path")),
                "Unknown"
            );
        }
        app.snapshot.applications_available = true;
    }
    app.snapshot.activity[1].path = None;
    let buffer = render(&app, 140, 40);
    assert_eq!(
        cell(&buffer, row(&buffer, "Terminal"), "Incoming", Some("Path")),
        "Unknown"
    );
}

#[test]
fn maximum_counters_keep_units_and_rate_suffixes_visible() {
    let mut app = fixture_app();
    let process = &mut app.snapshot.activity[0];
    process.rate_in = u64::MAX;
    process.rate_out = 100 * 1024 * 1024;
    process.bytes_in = u64::MAX;
    process.bytes_out = 100 * 1024 * 1024;
    for (width, height) in [(80, 24), (120, 34), (140, 40)] {
        let buffer = render(&app, width, height);
        let y = row(&buffer, "Safari");
        let download = cell(&buffer, y, "↓/s", Some("↑/s"));
        let upload = cell(&buffer, y, "↑/s", Some("↓ Total"));
        assert!(download.ends_with("EiB/s"), "{download:?}");
        assert!(upload.ends_with("MiB/s"), "{upload:?}");
        assert!(download.chars().count() <= 9);
        assert!(upload.chars().count() <= 9);
        assert!(cell(&buffer, y, "↓ Total", Some("↑ Total")).ends_with("EiB"));
        let next = (width >= 120).then_some("Incoming");
        assert!(cell(&buffer, y, "↑ Total", next).ends_with("MiB"));
    }
}

#[test]
fn narrow_ipv6_rows_preserve_inspection_and_sanitize_untrusted_text() {
    let mut app = fixture_app();
    app.snapshot.activity.truncate(1);
    let address = "2001:db8:1234:5678:9abc:def0:1234:5678";
    let process = &mut app.snapshot.activity[0];
    process.name = "Safari\u{1b}[2J\u{202e}".into();
    process.path = Some("/Applications/Safari\u{1b}\u{2066}.app".into());
    let peer = &mut process.connections[0];
    peer.remote_ip = format!("{address}\u{1b}\u{202e}");
    peer.country.as_mut().unwrap().code = "US\u{1b}\u{2066}".into();
    key(&mut app, KeyCode::Enter);
    for width in [50, 60, 80] {
        let buffer = render(&app, width, 24);
        let text = lines(&buffer).join("\n");
        assert!(!text.contains(address));
        let process_y = row(&buffer, "Safari");
        assert_eq!(
            compact(&cell(&buffer, process_y, "↓/s", Some("↑/s"))),
            "101B/s"
        );
        let last_rate = cell(
            &buffer,
            process_y,
            "↑/s",
            (width >= 80).then_some("↓ Total"),
        );
        assert_eq!(compact(&last_rate), "202B/s");
        assert!(
            text.contains('…'),
            "missing endpoint ellipsis at {width}:\n{text}"
        );
        for symbol in buffer.content.iter().map(|cell| cell.symbol()) {
            assert!(!symbol.chars().any(|ch| ch.is_control()
                || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')));
        }
    }
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    assert!(matches!(app.popup, Some(Popup::Inspect { .. })));
    let inspection = lines(&render(&app, 120, 34)).join("\n");
    assert!(inspection.contains(address));
    assert!(inspection.contains("port 443"));
    assert!(inspection.contains("tcp"));
}

#[test]
fn country_summaries_count_distinct_hidden_countries() {
    let mut app = fixture_app();
    let process = &mut app.snapshot.activity[0];
    let peer = process.connections[0].clone();
    process.connections = ["US", "DE", "FR", "JP", "DE", "US"]
        .into_iter()
        .map(|code| {
            let mut peer = peer.clone();
            peer.country.as_mut().unwrap().code = code.into();
            peer
        })
        .collect();
    for (width, height) in [(80, 24), (120, 34), (140, 40)] {
        let buffer = render(&app, width, height);
        let y = row(&buffer, "Safari");
        assert_eq!(cell(&buffer, y, "Peers", Some("Country")), "6");
        assert_eq!(cell(&buffer, y, "Country", Some("↓/s")), "DE, FR +2");
    }
    let process = &mut app.snapshot.activity[0];
    process.connections.truncate(2);
    process.connections[0].local = true;
    process.connections[1].country = None;
    let buffer = render(&app, 80, 24);
    assert_eq!(
        cell(&buffer, row(&buffer, "Safari"), "Country", Some("↓/s")),
        "Local +1"
    );
}

#[test]
fn truncated_names_keep_combining_characters_and_joined_emoji_together() {
    let mut app = fixture_app();
    app.snapshot.activity[0].name = format!("Glyph {}", "e\u{301}👩\u{200d}💻界".repeat(12));
    for (width, height) in [(50, 24), (60, 24), (80, 24), (120, 34), (140, 40)] {
        let buffer = render(&app, width, height);
        let y = row(&buffer, "Glyph");
        let first_column = header_column(&buffer, "APP / PEER");
        let next_column = header_column(&buffer, if width >= 80 { "Peers" } else { "Country" });
        let symbols = (first_column..next_column)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<Vec<_>>();
        assert!(
            symbols.contains(&"…"),
            "name should be truncated at {width}"
        );
        assert!(symbols.contains(&"e\u{301}"));
        assert!(symbols.contains(&"👩\u{200d}💻"));
        for symbol in symbols {
            if symbol.contains('👩') || symbol.contains('💻') || symbol.contains('\u{200d}') {
                assert_eq!(symbol, "👩\u{200d}💻");
            }
            if symbol.contains('\u{301}') {
                assert_eq!(symbol, "e\u{301}");
            }
        }
        assert_eq!(compact(&cell(&buffer, y, "↓/s", Some("↑/s"))), "101B/s");
    }
}

#[test]
fn an_empty_filtered_table_reports_no_matches_instead_of_waiting_for_traffic() {
    let mut app = fixture_app();
    app.filters[0] = "no-such-app-or-peer".into();
    assert!(!app.snapshot.activity.is_empty());
    for (width, height) in [(80, 24), (120, 34), (140, 40)] {
        let text = lines(&render(&app, width, height)).join("\n");
        assert!(text.contains("No apps or peers match this search."));
        assert!(!text.contains("Waiting for process statistics"));
    }
}

#[test]
fn wide_path_cells_preserve_app_names_and_complete_directory_components() {
    let mut app = fixture_app();
    app.resources_visible = false;
    let safari_path = app.snapshot.activity[0].path.as_deref().unwrap();
    for (width, height) in [(120, 34), (140, 40)] {
        let buffer = render(&app, width, height);
        let safari = row(&buffer, "Safari");
        let terminal = row(&buffer, "Terminal");
        let displayed_safari = cell(&buffer, safari, "Path", None);
        let displayed_terminal = cell(&buffer, terminal, "Path", None);
        assert!(displayed_safari.ends_with("/Safari.app"));
        assert!(displayed_terminal.ends_with("/Terminal.app"));
        if width == 120 {
            assert_eq!(displayed_safari, "…/Safari.app");
            assert_eq!(displayed_terminal, "…/Terminal.app");
        } else {
            assert_eq!(displayed_safari, safari_path);
            assert_eq!(displayed_terminal, "…/Utilities/Terminal.app");
        }
        assert_eq!(cell(&buffer, safari, "Incoming", Some("Path")), "Allow");
    }
    assert_eq!(app.snapshot.activity[0].path.as_deref(), Some(safari_path));
}

#[test]
fn expansion_markers_follow_visible_peers_and_manual_expansion() {
    let mut app = fixture_app();
    app.filters[0] = "app:Safari proto:tcp".into();
    for (width, height) in [(80, 24), (120, 34)] {
        let buffer = render(&app, width, height);
        assert!(lines(&buffer)[usize::from(row(&buffer, "Safari"))].contains("▾"));
        assert!(
            lines(&buffer)
                .iter()
                .any(|line| line.contains("151.101.1.69"))
        );
    }
    app.filters[0].clear();
    let buffer = render(&app, 80, 24);
    assert!(lines(&buffer)[usize::from(row(&buffer, "Safari"))].contains("▸"));
    app.expanded.insert(process_key(&app.snapshot.activity[0]));
    let buffer = render(&app, 80, 24);
    assert!(lines(&buffer)[usize::from(row(&buffer, "Safari"))].contains("▾"));
}

#[test]
fn an_open_inspector_tracks_the_sample_independently_of_filter_changes() {
    let mut app = fixture_app();
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    assert!(matches!(app.popup, Some(Popup::Inspect { .. })));
    app.filters[0] = "incoming:block".into();
    assert!(
        app.activity_rows()
            .iter()
            .all(|row| row.process().name != "Safari")
    );
    let buffer = render(&app, 80, 24);
    assert!(
        lines(&buffer)
            .iter()
            .any(|line| line.contains("151.101.1.69"))
    );
    app.snapshot.activity[0].connections.clear();
    let buffer = render(&app, 80, 24);
    assert!(
        lines(&buffer)
            .join("\n")
            .contains("no longer in the current sample")
    );
}

fn add_resources(app: &mut App) {
    use rooklet_core::resources::{ProcessReading, ReadingState, Usage};
    app.snapshot.resources.enabled = true;
    let process = &app.snapshot.activity[0];
    app.snapshot.resources.groups.insert(
        process.path.clone().unwrap(),
        Usage {
            process_count: 2,
            sampled_count: 2,
            cpu_percent: Some(125.5),
            memory_bytes: Some(64 * 1024 * 1024),
            read_per_sec: Some(4096),
            write_per_sec: Some(2048),
            state: ReadingState::Fresh,
            age_ms: Some(100),
            processes: process
                .identities
                .iter()
                .map(|identity| ProcessReading {
                    identity: identity.clone(),
                    cpu_percent: Some(62.75),
                    memory_bytes: Some(32 * 1024 * 1024),
                    read_per_sec: Some(2048),
                    write_per_sec: Some(1024),
                    state: ReadingState::Fresh,
                    age_ms: Some(100),
                })
                .collect(),
        },
    );
}

#[test]
fn resources_replace_paths_at_wide_sizes_and_leave_narrow_network_columns() {
    let mut app = fixture_app();
    add_resources(&mut app);
    for width in [80, 120, 160] {
        let buffer = render(&app, width, 34);
        let text = lines(&buffer).join("\n");
        if width == 80 {
            assert!(!text.contains("CPU %"));
        } else {
            let safari = row(&buffer, "Safari");
            assert_eq!(cell(&buffer, safari, "Procs", Some("CPU %")), "2");
            assert_eq!(cell(&buffer, safari, "CPU %", Some("Mem est")), "125.5%");
            assert_eq!(cell(&buffer, safari, "Mem est", None), "64MiB");
            assert!(!text.contains("Path"));
            assert!(text.contains("CPU 100% = 1 core"));
            assert_eq!(
                cell(&buffer, row(&buffer, "Terminal"), "CPU %", Some("Mem est")),
                "—"
            );
        }
        assert!(text.contains("↓ Total"));
        assert!(text.contains("↑ Total"));
    }
    key(&mut app, KeyCode::Char('r'));
    assert!(!app.resources_visible);
    assert!(lines(&render(&app, 120, 34)).join("\n").contains("Path"));
}

#[test]
fn partial_stale_warming_and_disabled_resources_are_explicit() {
    use rooklet_core::resources::ReadingState;
    let mut app = fixture_app();
    add_resources(&mut app);
    let resource_key = app.snapshot.activity[0].path.clone().unwrap();
    for state in [
        ReadingState::Partial,
        ReadingState::Stale,
        ReadingState::WarmingUp,
        ReadingState::Unavailable,
    ] {
        app.snapshot
            .resources
            .groups
            .get_mut(&resource_key)
            .unwrap()
            .state = state;
        let buffer = render(&app, 120, 34);
        let value = cell(&buffer, row(&buffer, "Safari"), "CPU %", Some("Mem est"));
        assert_eq!(
            cell(&buffer, row(&buffer, "Safari"), "Procs", Some("CPU %")),
            if matches!(state, ReadingState::Partial | ReadingState::Stale) {
                "~2"
            } else {
                "2"
            }
        );
        assert_eq!(
            value,
            if matches!(state, ReadingState::Partial | ReadingState::Stale) {
                "~125.5%"
            } else {
                "—"
            }
        );
        assert_eq!(
            cell(&buffer, row(&buffer, "Safari"), "Mem est", None),
            match state {
                ReadingState::Partial | ReadingState::Stale => "~64MiB",
                ReadingState::WarmingUp => "64MiB",
                _ => "—",
            }
        );
    }
    app.snapshot.activity[1].identities.clear();
    let buffer = render(&app, 120, 34);
    assert_eq!(
        cell(&buffer, row(&buffer, "Terminal"), "Procs", Some("CPU %")),
        "—"
    );
    app.snapshot.resources.enabled = false;
    let buffer = render(&app, 120, 34);
    assert_eq!(
        cell(&buffer, row(&buffer, "Safari"), "CPU %", Some("Mem est")),
        "—"
    );
}

#[test]
fn process_inspection_scrolls_cached_details_and_preserves_enter_expansion() {
    let mut app = fixture_app();
    app.snapshot.activity[0].identities[0]
        .path
        .push_str("\u{1b}\u{202e}");
    add_resources(&mut app);
    key(&mut app, KeyCode::Char('i'));
    assert!(matches!(app.popup, Some(Popup::Inspect { .. })));
    let buffer = render(&app, 120, 34);
    let text = lines(&buffer).join("\n");
    assert!(text.contains("PROCESS DETAILS"));
    assert!(text.contains("owner UID 1000"));
    assert!(text.contains("parent PID 1"));
    assert!(text.contains("Unix seconds"));
    assert!(text.contains("Disk read:"));
    assert!(!text.chars().any(|ch| ch == '\u{1b}' || ch == '\u{202e}'));
    key(&mut app, KeyCode::PageDown);
    assert!(matches!(&app.popup, Some(Popup::Inspect { scroll, .. }) if scroll.get() > 0));
    key(&mut app, KeyCode::End);
    assert!(
        lines(&render(&app, 80, 24))
            .join("\n")
            .contains("Esc closes")
    );
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Enter);
    assert!(
        app.expanded
            .contains(&process_key(&app.snapshot.activity[0]))
    );
    assert!(app.popup.is_none());
}

#[test]
fn resource_columns_share_row_hit_geometry_and_details_capture_wheel_events() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use rooklet::app::{MouseAction, View};
    let mut app = fixture_app();
    add_resources(&mut app);
    for width in [80, 120, 160] {
        let mut terminal = Terminal::new(TestBackend::new(width, 34)).unwrap();
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
        let x = if width >= 120 {
            header_column(buffer, "CPU %")
        } else {
            header_column(buffer, "Country")
        };
        let action = hits.unwrap().action(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: row(buffer, "Safari"),
            modifiers: KeyModifiers::NONE,
        });
        assert!(
            matches!(action, Some(MouseAction::Row { view: View::Activity, key, .. }) if key == process_key(&app.snapshot.activity[0]))
        );
    }
    key(&mut app, KeyCode::Char('i'));
    let mut terminal = Terminal::new(TestBackend::new(120, 34)).unwrap();
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
    assert!(matches!(
        hits.unwrap().action(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 40,
            row: 12,
            modifiers: KeyModifiers::NONE,
        }),
        Some(MouseAction::DialogScroll(3))
    ));
}

#[test]
fn resource_sort_enables_columns_and_disabling_resources_restores_observed_order() {
    use rooklet::app::ActivitySort;
    let mut app = fixture_app();
    app.resources_visible = false;
    app.activity_sort = ActivitySort::Peers;
    key(&mut app, KeyCode::Char('s'));
    assert_eq!(app.activity_sort, ActivitySort::Cpu);
    assert!(app.resources_visible);
    key(&mut app, KeyCode::Char('r'));
    assert_eq!(app.activity_sort, ActivitySort::Snapshot);
    assert!(!app.resources_visible);
}
