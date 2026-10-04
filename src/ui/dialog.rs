//! Modal help, inspectors, editors, and mutation confirmations.
use super::{HitMap, activity::country, mouse::shortcuts, theme::Palette};
use crate::app::{ActivityRow, App, ConfirmedAction, MouseAction, Popup};
use crate::presentation::{bytes, clean};
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Margin, Rect},
    style::Style,
    text::Line,
    widgets::{Clear, Paragraph, Wrap},
};

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(4));
    let height = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}
pub(super) fn draw(
    frame: &mut Frame,
    app: &App,
    popup: &Popup,
    area: Rect,
    p: Palette,
    hits: &mut HitMap,
) {
    let (title, height) = match popup {
        Popup::Help => ("KEYBOARD & MOUSE", 22),
        Popup::Inspect(_) => ("CONNECTION", 16),
        Popup::Confirm { title, action, .. } => (
            title.as_str(),
            if matches!(action, ConfirmedAction::Terminate(_)) {
                16
            } else if matches!(action, ConfirmedAction::Firewall(crate::model::Mutation::Applications { paths, .. }) if paths.len() > 1)
            {
                20
            } else {
                13
            },
        ),
        Popup::Application { .. } => ("ADD APPLICATION", 10),
        Popup::Network { .. } => ("MACHINE-WIDE NETWORK RULE", 19),
    };
    let rect = centered(area, 76, height);
    frame.render_widget(Clear, rect);
    let border = if matches!(popup, Popup::Confirm { action: ConfirmedAction::Terminate(request), .. } if request.mode == crate::process::TerminationMode::ForceKill)
    {
        p.bad
    } else {
        p.accent
    };
    let block = p.block(title).border_style(Style::default().fg(border));
    let inner = block.inner(rect).inner(Margin::new(1, 1));
    frame.render_widget(block, rect);
    let content = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(2),
    );
    let buttons =
        Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1).intersection(inner);
    let choices: &[(&str, KeyCode)] = match popup {
        Popup::Confirm {
            action: ConfirmedAction::Terminate(request),
            ..
        } => &[
            (
                match request.mode {
                    crate::process::TerminationMode::Terminate => "[Enter Terminate]",
                    crate::process::TerminationMode::ForceKill => "[Enter Force kill]",
                },
                KeyCode::Enter,
            ),
            ("[Esc Cancel]", KeyCode::Esc),
        ],
        Popup::Confirm { .. } => &[
            ("[Enter Apply]", KeyCode::Enter),
            ("[Esc Cancel]", KeyCode::Esc),
        ],
        Popup::Application { .. } | Popup::Network { .. } => &[
            ("[Enter Review]", KeyCode::Enter),
            ("[Esc Cancel]", KeyCode::Esc),
        ],
        Popup::Help | Popup::Inspect(_) => &[("[Esc Close]", KeyCode::Esc)],
    };
    let mut choices = choices.to_vec();
    if matches!(popup, Popup::Network { .. }) {
        for index in 0..7.min(content.height as usize) {
            hits.click(
                Rect::new(content.x, content.y + index as u16, content.width, 1),
                MouseAction::DialogField(index),
            );
        }
    }
    let lines = match popup {
        Popup::Help => vec![
            "Tab / 1–4     Activity, Applications, Network, Settings".into(),
            "↑↓ / j k      Move selection".into(),
            "Enter         Expand, inspect, edit, or toggle".into(),
            "/             Search current view".into(),
            "a / b         Allow / block INCOMING app connections".into(),
            "n             Add app path or machine-wide network rule".into(),
            "d / t         Delete / toggle network rule".into(),
            "+ / -         Raise / lower network rule precedence".into(),
            "Space         Freeze activity display".into(),
            "x / X         Terminate / force kill Activity app + helpers".into(),
            "Esc           Cancel dialog or clear search".into(),
            "u             Authenticate for PF status and changes".into(),
            "g in Settings Update the offline country database".into(),
            "q / Ctrl-C    Quit; applied firewall rules remain".into(),
            "Click selects · double-click opens · wheel moves".into(),
            "Click fields to focus; click choices to cycle".into(),
        ],
        Popup::Confirm { body, .. } => body.lines().map(str::to_owned).collect::<Vec<_>>(),
        Popup::Application { path } => vec![
            "Absolute .app bundle or executable path:".into(),
            "".into(),
            format!("{}█", clean(path)),
            "".into(),
            "Review opens a confirmation before applying.".into(),
        ],
        Popup::Network { draft, field } => {
            let values = [
                ("Remote IP/CIDR", draft.destination.clone()),
                (
                    "Destination port",
                    if draft.port.is_empty() {
                        "any".into()
                    } else {
                        draft.port.clone()
                    },
                ),
                ("Protocol", draft.protocol.to_string()),
                ("Direction", draft.direction.to_string()),
                ("Action", draft.action.to_string()),
                (
                    "Interface",
                    if draft.interface.is_empty() {
                        "any".into()
                    } else {
                        draft.interface.clone()
                    },
                ),
                ("Name", draft.name.clone()),
            ];
            let mut lines = values
                .iter()
                .enumerate()
                .map(|(index, (label, value))| {
                    format!(
                        "{} {:<19} {}",
                        if index == *field { "›" } else { " " },
                        label,
                        clean(value)
                    )
                })
                .collect::<Vec<_>>();
            lines.extend([
                "".into(),
                "Tab / ↑↓ field · ←→ / Space changes choice".into(),
                "Type edits text · Enter reviews · Esc cancels".into(),
                "".into(),
                "Applies to every app. Port is local for inbound traffic.".into(),
            ]);
            lines
        }
        Popup::Inspect(key) => {
            if let Some(ActivityRow::Connection(process, flow)) =
                app.activity_rows().iter().find(|row| row.key() == *key)
            {
                vec![
                    clean(&process.name),
                    format!(
                        "{} · port {} · {}",
                        clean(&flow.remote_ip),
                        flow.remote_port
                            .map(|p| p.to_string())
                            .unwrap_or_else(|| "unknown".into()),
                        flow.protocol
                    ),
                    country(flow),
                    "".into(),
                    format!("↓ {} received", bytes(flow.bytes_in)),
                    format!("↑ {} sent", bytes(flow.bytes_out)),
                    "".into(),
                    "Observed peer; firewall verdict is not available.".into(),
                    "Country is an estimate for the observed IP.".into(),
                    "".into(),
                    "Click Close or press Esc to return.".into(),
                ]
            } else {
                vec![
                    "Connection is no longer in the current sample.".into(),
                    "Click Close or press Esc to return.".into(),
                ]
            }
        }
    };
    let text = lines
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            if let Popup::Network { field, .. } = popup
                && index == *field
            {
                Line::styled(text, Style::default().fg(p.accent).bg(p.selection))
            } else {
                Line::raw(text)
            }
        })
        .collect::<Vec<_>>();
    let paragraph = Paragraph::new(text).style(Style::default().fg(p.text));
    // Editor fields occupy one terminal row each; clipping keeps pointer focus
    // aligned with their visible labels even for long values and narrow windows.
    let paragraph = if matches!(popup, Popup::Network { .. }) {
        paragraph
    } else {
        paragraph.wrap(Wrap { trim: true })
    };
    let paragraph = if let Popup::Confirm { scroll, .. } = popup {
        let max = paragraph
            .line_count(content.width)
            .saturating_sub(content.height.into())
            .min(u16::MAX as usize) as u16;
        scroll.set(scroll.get().min(max));
        if max > 0 {
            choices.push(("[↓ More]", KeyCode::Down));
        }
        hits.dialog_scroll(content);
        paragraph.scroll((scroll.get(), 0))
    } else {
        paragraph
    };
    frame.render_widget(paragraph, content);
    shortcuts(frame, buttons, &choices, p, hits);
}
