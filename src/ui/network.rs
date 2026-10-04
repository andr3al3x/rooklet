//! Machine-wide PF rule table and setup/status states.
use super::{HitMap, theme::Palette};
use crate::app::{App, View};
use crate::model::Action;
use crate::presentation::clean;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    widgets::{Cell, Paragraph, Row, Table, TableState, Wrap},
};

pub(super) fn draw(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    p: Palette,
    hits: &mut HitMap,
    state: &mut TableState,
) {
    let parts = Layout::vertical([Constraint::Length(2), Constraint::Min(2)]).split(area);
    let status = &app.snapshot.network;
    let message = status.message.as_deref().unwrap_or(if status.applied {
        "Rules loaded in PF. Existing connections may continue."
    } else {
        "Rules are not confirmed applied."
    });
    frame.render_widget(
        Paragraph::new(clean(message))
            .style(p.muted())
            .wrap(Wrap { trim: true }),
        parts[0],
    );
    if !status.rules_available {
        super::message(
            frame,
            parts[1],
            "NETWORK STATUS",
            concat!(
                "Saved network rules need administrator access. Press u to authenticate, ",
                "then let this screen refresh. No PF settings are changed by reading status."
            ),
            p,
        );
        return;
    }
    if !status.configured {
        super::message(
            frame,
            parts[1],
            "NETWORK RULES",
            concat!(
                "Optional machine-wide IP and port filtering.\n\nSet up the dedicated PF anchor:\n",
                "  sudo xield network setup\n\nApplication-specific outgoing rules are unavailable."
            ),
            p,
        );
        return;
    }
    let rows = app.rules();
    if rows.is_empty() {
        super::message(
            frame,
            parts[1],
            "NETWORK · FIRST MATCH WINS",
            "No network rules. Press n to create an IP/CIDR rule. Rules apply to all applications.",
            p,
        );
        return;
    }
    let data = rows
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            Row::new(vec![
                Cell::from(format!(
                    "{:02} {}",
                    index + 1,
                    if rule.enabled { "on" } else { "off" }
                )),
                Cell::from(clean(&rule.name)),
                Cell::from(format!(
                    "{}{}",
                    clean(&rule.destination),
                    rule.port.map(|port| format!(":{port}")).unwrap_or_default()
                )),
                Cell::from(format!("{} {}", rule.protocol, rule.direction)).style(p.muted()),
                Cell::from(rule.action.to_string()).style(Style::default().fg(
                    if rule.action == Action::Block {
                        p.bad
                    } else {
                        p.good
                    },
                )),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_stateful_widget(
        Table::new(
            data,
            [
                Constraint::Length(6),
                Constraint::Min(14),
                Constraint::Min(20),
                Constraint::Length(9),
                Constraint::Length(5),
            ],
        )
        .header(
            Row::new(["ORDER", "NAME", "PEER / PORT", "SCOPE", "ACTION"])
                .style(p.muted())
                .bottom_margin(1),
        )
        .block(p.block("NETWORK · FIRST MATCH WINS"))
        .column_spacing(1)
        .row_highlight_style(Style::default().bg(p.selection))
        .highlight_symbol("› "),
        parts[1],
        &mut *state,
    );
    let keys = rows.iter().map(|rule| rule.id.clone()).collect::<Vec<_>>();
    hits.table(parts[1], 2, state.offset(), View::Network, &keys);
}
