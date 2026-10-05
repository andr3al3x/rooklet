//! Profile list viewport and pointer geometry share the same row offset.
use super::super::{HitMap, theme::Palette};
use crate::app::MouseAction;
use ratatui::{Frame, layout::Rect, style::Style, text::Line, widgets::Paragraph};
use rooklet_core::text::clean;

pub(super) fn list(
    frame: &mut Frame,
    entries: &[String],
    selected: usize,
    loading: bool,
    area: Rect,
    p: Palette,
    hits: &mut HitMap,
) {
    if loading || entries.is_empty() {
        frame.render_widget(
            Paragraph::new(if loading {
                "Loading profile…"
            } else {
                "No managed profiles.\n\nPress e to export current firewall scopes."
            })
            .style(p.muted()),
            area,
        );
        return;
    }
    let header = area.height.min(3);
    frame.render_widget(
        Paragraph::new(
            "Enter: review all scopes before applying.\ne: export current settings; no overwrite.",
        )
        .style(p.muted()),
        Rect::new(area.x, area.y, area.width, header),
    );
    let body = Rect::new(area.x, area.y + header, area.width, area.height - header);
    if body.is_empty() {
        return;
    }
    let offset = selected.saturating_sub(usize::from(body.height).saturating_sub(1));
    hits.dialog_scroll(body);
    for (row, (index, name)) in entries
        .iter()
        .enumerate()
        .skip(offset)
        .take(body.height.into())
        .enumerate()
    {
        let rect = Rect::new(body.x, body.y + row as u16, body.width, 1);
        let line = Line::raw(format!(
            "{} {}",
            if index == selected { "›" } else { " " },
            clean(name)
        ));
        frame.render_widget(
            Paragraph::new(line).style(if index == selected {
                Style::default().fg(p.accent).bg(p.selection)
            } else {
                Style::default().fg(p.text)
            }),
            rect,
        );
        hits.click(rect, MouseAction::ProfileRow(index));
    }
}
