//! Explicit developer export of fixture-backed Ratatui cells, never system state.
mod common;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Terminal,
    backend::TestBackend,
    style::{Color, Modifier},
};
use rooklet::{
    app::{App, ProfileOutcome, View},
    model::{Profile, Snapshot},
    profile,
    ui::{self, Theme},
};
use serde_json::json;
use std::path::PathBuf;

fn color(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => "#d6e0eb".into(),
    }
}

/// Run explicitly with `cargo test --test visual_preview -- --ignored`, then
/// pass the exported JSON to scripts/render-preview.py for visual inspection.
#[test]
#[ignore = "exports terminal-cell fixtures to target/visual-preview for manual inspection"]
fn export_terminal_cells() {
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/visual-preview");
    std::fs::create_dir_all(&output).unwrap();
    for (name, width, height) in [
        ("activity", 120, 34),
        ("activity", 80, 24),
        ("network-filtered", 120, 34),
        ("network-filtered", 80, 24),
        ("settings", 120, 34),
        ("unavailable", 80, 24),
        ("profiles", 120, 34),
        ("profiles", 80, 24),
        ("profiles", 50, 17),
        ("profile-name", 80, 24),
        ("profile-name", 50, 17),
        ("profile-review", 120, 34),
        ("profile-review", 50, 17),
    ] {
        let mut app = App::new(if name == "unavailable" {
            Snapshot::default()
        } else {
            common::snapshot()
        });
        if name == "settings" {
            app.view = View::Settings;
        } else if name == "network-filtered" {
            app.view = View::Network;
            let mut second = app.snapshot.network.rules[0].clone();
            second.id = "second-rule".into();
            second.name = "Second target".into();
            app.snapshot.network.rules.push(second);
            app.filters[2] = "Second target".into();
            app.handle(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        } else if name == "activity" {
            app.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
        if name.starts_with("profile") {
            app.handle(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
            app.profiles_finished(
                common::snapshot(),
                ProfileOutcome::Listed(vec!["Home".into(), "Public WiFi".into(), "Work".into()]),
            );
            if name == "profile-name" {
                app.handle(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
            } else if name == "profile-review" {
                app.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                let before = common::snapshot();
                let mut proposed = Profile::from_snapshot(&before);
                proposed.applications.clear();
                proposed.network_rules.clear();
                proposed.firewall.as_mut().unwrap().stealth = true;
                app.profiles_finished(
                    before.clone(),
                    ProfileOutcome::Prepared {
                        name: "Home".into(),
                        prepared: Box::new(profile::prepare(&proposed, &before).unwrap()),
                    },
                );
            }
        }
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| ui::draw(frame, &app, Theme::Dark))
            .unwrap();
        let cells = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| {
                json!({
                    "symbol": cell.symbol(),
                    "fg": color(cell.fg),
                    "bg": color(cell.bg),
                    "bold": cell.modifier.contains(Modifier::BOLD),
                })
            })
            .collect::<Vec<_>>();
        std::fs::write(
            output.join(format!("{name}-{width}x{height}.json")),
            serde_json::to_vec(&json!({"width": width, "height": height, "cells": cells})).unwrap(),
        )
        .unwrap();
    }
}
