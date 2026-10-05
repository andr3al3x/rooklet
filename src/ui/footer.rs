//! Context-sensitive shortcuts wrapped using rendered terminal-cell widths.
use super::{HitMap, mouse::shortcuts, theme::Palette};
use crate::{
    app::{App, View},
    presentation::clean,
};
use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect, style::Style, text::Line, widgets::Paragraph};
type Choice = (&'static str, KeyCode);
fn choices(view: View) -> &'static [Choice] {
    match view {
        View::Activity => &[
            ("[Enter Expand]", KeyCode::Enter),
            ("[a Allow]", KeyCode::Char('a')),
            ("[b Block]", KeyCode::Char('b')),
            ("[n IP rule]", KeyCode::Char('n')),
            ("[s Sort]", KeyCode::Char('s')),
            ("[Space Freeze]", KeyCode::Char(' ')),
            ("[x Terminate]", KeyCode::Char('x')),
            ("[X Force kill]", KeyCode::Char('X')),
        ],
        View::Applications => &[
            ("[a Allow]", KeyCode::Char('a')),
            ("[b Block]", KeyCode::Char('b')),
            ("[n Add]", KeyCode::Char('n')),
            ("[d Remove]", KeyCode::Char('d')),
        ],
        View::Network => &[
            ("[n Add]", KeyCode::Char('n')),
            ("[Enter Edit]", KeyCode::Enter),
            ("[t Toggle]", KeyCode::Char('t')),
            ("[d Delete]", KeyCode::Char('d')),
            ("[+ Up]", KeyCode::Char('+')),
            ("[- Down]", KeyCode::Char('-')),
            ("[w Explain]", KeyCode::Char('w')),
        ],
        View::Settings => &[
            ("[Enter Toggle]", KeyCode::Enter),
            ("[g Update countries]", KeyCode::Char('g')),
        ],
    }
}
fn rows(app: &App, width: u16) -> Vec<Vec<Choice>> {
    let mut rows = vec![Vec::new()];
    let mut used = 0;
    for &choice in choices(app.view) {
        let size = Line::raw(choice.0).width() as u16;
        let gap = if used == 0 { 0 } else { 2 };
        if used > 0 && used + gap + size > width {
            rows.push(Vec::new());
            used = 0;
        }
        used += if used == 0 { size } else { size + 2 };
        rows.last_mut().unwrap().push(choice);
    }
    rows
}
pub(super) fn height(app: &App, width: u16) -> u16 {
    if app.searching || !app.filter().is_empty() {
        3
    } else {
        rows(app, width).len() as u16 + 2
    }
}
pub(super) fn draw(frame: &mut Frame, app: &App, area: Rect, p: Palette, hits: &mut HitMap) {
    let row =
        |offset| Rect::new(area.x, area.y.saturating_add(offset), area.width, 1).intersection(area);
    let toolbar_height = height(app, area.width) - 2;
    if app.searching || !app.filter().is_empty() {
        let text = if app.searching {
            format!("Search: {}█", clean(app.filter()))
        } else {
            format!("Filter: {}  ·  Esc clears", clean(app.filter()))
        };
        frame.render_widget(Paragraph::new(text).style(p.muted()), row(0));
    } else {
        for (offset, choices) in rows(app, area.width).iter().enumerate() {
            shortcuts(frame, row(offset as u16), choices, p, hits);
        }
    }
    shortcuts(
        frame,
        row(toolbar_height),
        &[
            ("[/ Search]", KeyCode::Char('/')),
            ("[u Unlock]", KeyCode::Char('u')),
            ("[? Help]", KeyCode::Char('?')),
            ("[q Quit]", KeyCode::Char('q')),
        ],
        p,
        hits,
    );
    let filter_error = if app.view == View::Activity {
        app.activity_filter_error()
    } else {
        None
    };
    let (last, color) = if let Some(notice) = &app.notice
        && notice.at.elapsed().as_secs() < 8
    {
        (
            notice.text.clone(),
            if notice.error { p.bad } else { p.good },
        )
    } else if let Some(error) = filter_error {
        (error, p.bad)
    } else if app.snapshot.demo {
        ("SIMULATED ACTIVITY · no settings changed".into(), p.warn)
    } else {
        (
            "Observed traffic · country estimates · no payload capture".into(),
            p.muted,
        )
    };
    frame.render_widget(
        Paragraph::new(last).style(Style::default().fg(color)),
        row(toolbar_height + 1),
    );
}
