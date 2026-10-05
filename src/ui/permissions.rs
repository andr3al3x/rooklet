//! Incoming application permissions and firewall settings.
use super::{HitMap, theme::Palette};
use crate::app::{App, SETTINGS, View};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Cell, Paragraph, Row, Table, TableState, Wrap},
};
use rooklet_core::text::clean;

pub(super) fn applications(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    p: Palette,
    hits: &mut HitMap,
    state: &mut TableState,
) {
    let rows = app.applications();
    if rows.is_empty() {
        let text = if app.snapshot.applications_available {
            concat!(
                "No application entries. Press n to add an application path. ",
                "These permissions control incoming connections."
            )
        } else {
            "Incoming application entries unavailable. See Settings for diagnostics."
        };
        super::message(frame, area, "INCOMING APPLICATION PERMISSIONS", text, p);
        return;
    }
    let data = rows
        .iter()
        .map(|application| {
            Row::new(vec![
                Cell::from(clean(&application.name)),
                Cell::from(if application.blocked {
                    "block incoming"
                } else {
                    "allow incoming"
                })
                .style(Style::default().fg(if application.blocked {
                    p.bad
                } else {
                    p.good
                })),
                Cell::from(clean(&application.path)).style(p.muted()),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_stateful_widget(
        Table::new(
            data,
            [
                Constraint::Length(23),
                Constraint::Length(16),
                Constraint::Min(20),
            ],
        )
        .header(
            Row::new(["APPLICATION", "PERMISSION", "PATH"])
                .style(p.muted())
                .bottom_margin(1),
        )
        .block(p.block("INCOMING APPLICATION PERMISSIONS"))
        .column_spacing(1)
        .row_highlight_style(Style::default().bg(p.selection))
        .highlight_symbol("› "),
        area,
        &mut *state,
    );
    let keys = rows
        .iter()
        .map(|application| application.path.clone())
        .collect::<Vec<_>>();
    hits.table(area, 2, state.offset(), View::Applications, &keys);
}
pub(super) fn settings(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    p: Palette,
    hits: &mut HitMap,
    state: &mut TableState,
) {
    let parts = Layout::vertical([Constraint::Length(7), Constraint::Min(1)]).split(area);
    let data = SETTINGS
        .iter()
        .map(|(setting, name)| {
            Row::new(vec![
                Cell::from(*name),
                Cell::from(
                    app.snapshot
                        .firewall
                        .as_ref()
                        .map(|settings| if settings.get(*setting) { "on" } else { "off" })
                        .unwrap_or("unavailable"),
                )
                .style(p.muted()),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_stateful_widget(
        Table::new(data, [Constraint::Min(25), Constraint::Length(12)])
            .block(p.block("INCOMING FIREWALL · ENTER TO TOGGLE"))
            .column_spacing(2)
            .row_highlight_style(Style::default().bg(p.selection))
            .highlight_symbol("› "),
        parts[0],
        &mut *state,
    );
    let keys = SETTINGS
        .iter()
        .map(|(setting, _)| setting.to_string())
        .collect::<Vec<_>>();
    hits.table(parts[0], 0, state.offset(), View::Settings, &keys);
    let mut text = vec![
        Line::styled("STATUS & DIAGNOSTICS", p.muted()),
        Line::raw(format!(
            "PF: {} · {}",
            if app.snapshot.network.configured {
                "configured"
            } else {
                "not configured"
            },
            if app.snapshot.network.enabled {
                "enabled"
            } else {
                "not confirmed enabled"
            }
        )),
        Line::raw(format!(
            "Country database: {}",
            app.snapshot
                .geoip
                .as_deref()
                .map(clean)
                .unwrap_or_else(|| "not installed; press g to download".into())
        )),
    ];
    if app.snapshot.geoip.is_some() {
        text.push(Line::raw("IP geolocation by DB-IP · db-ip.com · CC BY 4.0"));
    }
    for notice in &app.snapshot.notices {
        text.push(Line::raw(clean(notice)));
    }
    if let Some(message) = &app.snapshot.network.message {
        text.push(Line::raw(clean(message)));
    }
    frame.render_widget(
        Paragraph::new(text)
            .style(p.muted())
            .wrap(Wrap { trim: true }),
        parts[1],
    );
}
