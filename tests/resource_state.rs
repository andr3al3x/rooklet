mod common;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use rooklet::{
    app::{ActivitySort, App, MouseAction, Popup, View},
    ui::{self, State, Theme},
};
use rooklet_core::resources::{ProcessReading, ReadingState, Resources, Usage};

fn reading_snapshot() -> rooklet_core::model::Snapshot {
    let mut snapshot = common::snapshot();
    snapshot.resources = Resources {
        enabled: true,
        ..Default::default()
    };
    for (index, process) in snapshot.activity.iter().enumerate() {
        let identity = process.identities[0].clone();
        let key = identity
            .bundle_path
            .clone()
            .unwrap_or_else(|| format!("pid:{}", identity.pid));
        snapshot.resources.groups.insert(
            key,
            Usage {
                process_count: process.identities.len(),
                sampled_count: process.identities.len(),
                cpu_percent: Some(10.0 * index as f64),
                memory_bytes: Some(1024 * (index as u64 + 1)),
                read_per_sec: Some(0),
                write_per_sec: Some(0),
                state: ReadingState::Fresh,
                age_ms: Some(0),
                processes: vec![ProcessReading {
                    identity,
                    cpu_percent: Some(10.0 * index as f64),
                    memory_bytes: Some(1024 * (index as u64 + 1)),
                    read_per_sec: Some(0),
                    write_per_sec: Some(0),
                    state: ReadingState::Fresh,
                    age_ms: Some(0),
                }],
            },
        );
    }
    snapshot
}

#[test]
fn collection_follows_visibility_freeze_and_resource_sorting() {
    let mut app = App::new(reading_snapshot());
    let state = State::default();
    let narrow = Rect::new(0, 0, 80, 24);
    let wide = Rect::new(0, 0, 120, 34);
    assert!(!state.resource_interest(&app, narrow).enabled);
    let interest = state.resource_interest(&app, wide);
    assert!(interest.enabled);
    assert_eq!(interest.tracked_pids, [201, 202, 203]);
    assert_eq!(interest.priority_pids.first(), Some(&201));
    app.view = View::Settings;
    assert!(!state.resource_interest(&app, wide).enabled);
    app.view = View::Activity;
    app.paused = true;
    assert!(!state.resource_interest(&app, wide).enabled);
    app.paused = false;
    app.activity_sort = ActivitySort::Cpu;
    assert!(state.resource_interest(&app, narrow).enabled);
    app.popup = Some(Popup::Help);
    assert!(!state.resource_interest(&app, wide).enabled);
    app.popup = None;
    assert!(
        !state
            .resource_interest(&app, Rect::new(0, 0, 120, 10))
            .enabled
    );
    app.resources_visible = false;
    assert!(!state.resource_interest(&app, narrow).enabled);
}

#[test]
fn selected_process_details_enable_narrow_collection_and_scroll_with_mouse() {
    let mut app = App::new(reading_snapshot());
    app.handle(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
    let state = State::default();
    assert!(
        state
            .resource_interest(&app, Rect::new(0, 0, 80, 24))
            .enabled
    );
    app.handle_mouse(MouseAction::DialogScroll(3));
    let Some(Popup::Inspect { scroll, .. }) = &app.popup else {
        panic!("missing details")
    };
    assert_eq!(scroll.get(), 3);
}

#[test]
fn freeze_preserves_readings_and_resume_uses_latest_resources() {
    let mut app = App::new(reading_snapshot());
    app.handle(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    let mut updated = reading_snapshot();
    updated
        .resources
        .groups
        .get_mut("/Applications/Safari.app")
        .unwrap()
        .memory_bytes = Some(8192);
    app.update(updated, false);
    assert_eq!(
        app.snapshot.resources.groups["/Applications/Safari.app"].memory_bytes,
        Some(1024)
    );
    app.handle(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    assert_eq!(
        app.snapshot.resources.groups["/Applications/Safari.app"].memory_bytes,
        Some(8192)
    );
    app.invalidate_observation("monitor failed");
    assert!(!app.snapshot.resources.enabled);
    assert!(app.snapshot.control_age_ms.is_none());
    assert!(app.snapshot.resources.groups.is_empty());
}

#[test]
fn resource_sorts_place_complete_readings_before_missing_or_partial_values() {
    let mut app = App::new(reading_snapshot());
    app.snapshot
        .resources
        .groups
        .get_mut("pid:203")
        .unwrap()
        .state = ReadingState::Partial;
    app.activity_sort = ActivitySort::Cpu;
    let pids: Vec<_> = app
        .activity_rows()
        .into_iter()
        .map(|row| row.process().pid)
        .collect();
    assert_eq!(pids, [202, 201, 203]);
    app.activity_sort = ActivitySort::Memory;
    let pids: Vec<_> = app
        .activity_rows()
        .into_iter()
        .map(|row| row.process().pid)
        .collect();
    assert_eq!(pids, [202, 201, 203]);
    app.snapshot
        .resources
        .groups
        .get_mut("/System/Applications/Utilities/Terminal.app")
        .unwrap()
        .cpu_percent = Some(f64::NAN);
    app.activity_sort = ActivitySort::Cpu;
    assert_eq!(app.activity_rows()[0].process().pid, 201);
}

#[test]
fn rendered_viewport_prioritizes_the_selected_process() {
    let mut app = App::new(reading_snapshot());
    app.selection[0] = Some(rooklet::app::process_key(&app.snapshot.activity[2]));
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    let mut state = State::default();
    terminal
        .draw(|frame| {
            ui::draw_interactive(frame, &app, Theme::Dark, &mut state);
        })
        .unwrap();
    let interest = state.resource_interest(&app, Rect::new(0, 0, 120, 24));
    assert_eq!(interest.priority_pids.first(), Some(&203));
}
