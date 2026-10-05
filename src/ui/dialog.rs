//! Modal help, inspectors, editors, and mutation confirmations.
mod information;
mod rules;
use super::{HitMap, mouse::shortcuts, theme::Palette};
use crate::app::{App, ConfirmedAction, Popup};
use crate::presentation::clean;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Margin, Rect},
    style::Style,
    text::Line,
    widgets::{Clear, Paragraph, Wrap},
};
use std::cell::Cell;

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
        Popup::Help => ("KEYBOARD & MOUSE", 24),
        Popup::Inspect(_) => ("CONNECTION", 16),
        Popup::Confirm { title, action, .. } => (
            title.as_str(),
            if matches!(action, ConfirmedAction::Terminate(_)) {
                16
            } else if matches!(action, ConfirmedAction::Firewall(crate::model::Mutation::Applications { paths, .. }) if paths.len() > 1)
                || matches!(
                    action,
                    ConfirmedAction::Firewall(crate::model::Mutation::NetworkRules(_))
                )
            {
                20
            } else {
                13
            },
        ),
        Popup::Application { .. } => ("ADD APPLICATION", 10),
        Popup::Network { .. } => ("MACHINE-WIDE NETWORK RULE", 19),
        Popup::Explain { .. } => ("EXPLAIN XIELD RULES · PREDICTION ONLY", 23),
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
        Popup::Help | Popup::Inspect(_) | Popup::Explain { .. } => &[("[Esc Close]", KeyCode::Esc)],
    };
    let mut choices = choices.to_vec();
    let more = match popup {
        Popup::Network { draft, field } => {
            rules::edit(frame, draft, *field, content, p, hits);
            false
        }
        Popup::Explain { draft, field } => {
            rules::explain(frame, app, draft, *field, content, p, hits);
            false
        }
        Popup::Help => render_text(
            frame,
            content,
            information::help().into_iter().map(Line::raw).collect(),
            p,
            None,
        ),
        Popup::Inspect(key) => render_text(
            frame,
            content,
            information::inspect(app, key)
                .into_iter()
                .map(Line::raw)
                .collect(),
            p,
            None,
        ),
        Popup::Application { path } => render_text(
            frame,
            content,
            vec![
                Line::raw("Absolute .app bundle or executable path:"),
                Line::raw(""),
                Line::raw(format!("{}█", clean(path))),
                Line::raw(""),
                Line::raw("Review opens a confirmation before applying."),
            ],
            p,
            None,
        ),
        Popup::Confirm { body, scroll, .. } => {
            hits.dialog_scroll(content);
            let lines = body
                .lines()
                .map(|text| {
                    if text.starts_with("Shadow warnings")
                        || text.contains(" is fully shadowed by ")
                    {
                        Line::styled(text, Style::default().fg(p.warn))
                    } else {
                        Line::raw(text)
                    }
                })
                .collect();
            render_text(frame, content, lines, p, Some(scroll))
        }
    };
    if more {
        choices.push(("[↓ More]", KeyCode::Down));
    }
    shortcuts(frame, buttons, &choices, p, hits);
}

fn render_text(
    frame: &mut Frame,
    area: Rect,
    lines: Vec<Line<'_>>,
    p: Palette,
    scroll: Option<&Cell<u16>>,
) -> bool {
    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(p.text))
        .wrap(Wrap { trim: true });
    let max = if scroll.is_some() {
        paragraph
            .line_count(area.width)
            .saturating_sub(area.height.into())
            .min(u16::MAX as usize) as u16
    } else {
        0
    };
    let paragraph = if let Some(scroll) = scroll {
        scroll.set(scroll.get().min(max));
        paragraph.scroll((scroll.get(), 0))
    } else {
        paragraph
    };
    frame.render_widget(paragraph, area);
    max > 0
}
