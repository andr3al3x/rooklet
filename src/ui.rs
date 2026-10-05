//! View composition; rendering details stay with each view.
mod activity;
mod chrome;
mod dialog;
mod footer;
mod mouse;
mod network;
mod permissions;
mod theme;

use crate::app::{App, MouseAction, View};
pub use mouse::HitMap;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    text::Line,
    widgets::{Block, Paragraph, TableState, Tabs, Wrap},
};
use theme::Palette;
pub use theme::Theme;

/// Persistent viewports keep rows under the pointer stable across redraws.
#[derive(Default)]
pub struct State {
    tables: [TableState; 4],
}

impl State {
    /// Match collection to the rendered viewport. Hidden resource panels do no counter work.
    pub fn resource_interest(
        &self,
        app: &App,
        area: Rect,
    ) -> rooklet_core::resources::ResourceInterest {
        let inspected = match &app.popup {
            Some(crate::app::Popup::Inspect { key, .. }) => app
                .snapshot
                .activity
                .iter()
                .find(|process| crate::app::process_key(process) == *key),
            _ => None,
        };
        let enabled = app.view == View::Activity
            && !app.paused
            && app.resources_visible
            && area.height >= 17
            && (inspected.is_some()
                || (app.popup.is_none()
                    && (area.width >= 120 || app.activity_sort.uses_resources())));
        if !enabled {
            return rooklet_core::resources::ResourceInterest::default();
        }
        let rows = app.activity_rows();
        let mut priority_pids = Vec::new();
        if let Some(process) = inspected {
            priority_pids.push(process.pid);
        }
        if let Some(index) = app.selected_index()
            && let Some(row) = rows.get(index)
        {
            priority_pids.push(row.process().pid);
        }
        for row in rows
            .iter()
            .skip(self.tables[0].offset())
            .take(usize::from(area.height).min(128))
        {
            let pid = row.process().pid;
            if !priority_pids.contains(&pid) {
                priority_pids.push(pid);
            }
        }
        rooklet_core::resources::ResourceInterest {
            enabled,
            tracked_pids: app
                .snapshot
                .activity
                .iter()
                .map(|process| process.pid)
                .take(4096)
                .collect(),
            priority_pids,
        }
    }
}

pub fn draw(frame: &mut Frame, app: &App, theme: Theme) {
    draw_interactive(frame, app, theme, &mut State::default());
}

pub fn draw_interactive(frame: &mut Frame, app: &App, theme: Theme, state: &mut State) -> HitMap {
    let p = Palette::new(theme);
    let area = frame.area();
    let mut hits = HitMap::new(area);
    frame.render_widget(Block::default().style(p.style()), area);
    if area.width < 50 || area.height < 17 {
        frame.render_widget(
            Paragraph::new("ROOKLET\n\nUse a terminal at least 50 × 17.\nPress q to quit.")
                .style(p.style()),
            area.inner(Margin::new(2, 1)),
        );
        return hits;
    }
    let outer = area.inner(Margin::new(2, 1));
    let parts = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Length(4),
        Constraint::Min(4),
        Constraint::Length(footer::height(app, outer.width)),
    ])
    .split(outer);
    chrome::header(frame, app, parts[0], p);
    frame.render_widget(
        Tabs::new(View::ALL.map(View::title))
            .select(app.view.index())
            .style(p.muted())
            .highlight_style(p.accent())
            .divider("   ")
            .padding("", ""),
        parts[1],
    );
    let mut x = parts[1].x;
    for view in View::ALL {
        let width = Line::raw(view.title()).width() as u16;
        let visible = Rect::new(x, parts[1].y, width, 1).intersection(parts[1]);
        hits.click(visible, MouseAction::View(view));
        x = x.saturating_add(width + 3);
    }
    chrome::summary(frame, app, parts[2], p);
    let table = &mut state.tables[app.view.index()];
    table.select(app.selected_index());
    match app.view {
        View::Activity => activity::draw(frame, app, parts[3], p, &mut hits, table),
        View::Applications => permissions::applications(frame, app, parts[3], p, &mut hits, table),
        View::Network => network::draw(frame, app, parts[3], p, &mut hits, table),
        View::Settings => permissions::settings(frame, app, parts[3], p, &mut hits, table),
    }
    footer::draw(frame, app, parts[4], p, &mut hits);
    if let Some(popup) = &app.popup {
        hits.modal();
        dialog::draw(frame, app, popup, area, p, &mut hits);
    }
    hits
}

fn message(frame: &mut Frame, area: Rect, title: &str, text: &str, palette: Palette) {
    frame.render_widget(
        Paragraph::new(rooklet_core::text::clean(text))
            .block(palette.block(title))
            .style(palette.muted())
            .wrap(Wrap { trim: true }),
        area,
    );
}
