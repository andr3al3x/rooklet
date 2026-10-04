//! Header and transfer summary.
use super::theme::Palette;
use crate::app::App;
use crate::presentation::bytes;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Sparkline, Wrap},
};

pub(super) fn header(frame: &mut Frame, app: &App, area: Rect, p: Palette) {
    let columns = Layout::horizontal([Constraint::Min(25), Constraint::Length(18)]).split(area);
    let (state, color) = if app.snapshot.demo {
        ("DEMO · no firewall changes".to_string(), p.warn)
    } else if app.stale() {
        ("Status stale".into(), p.warn)
    } else {
        match &app.snapshot.firewall {
            Some(s) if s.enabled => ("Incoming firewall on".into(), p.good),
            Some(_) => ("Incoming firewall off".into(), p.warn),
            None => ("Firewall unavailable".into(), p.bad),
        }
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("XIELD", p.accent()),
            Span::raw("   "),
            Span::styled(state, Style::default().fg(color)),
        ])),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(if app.busy {
            "working…"
        } else if app.paused {
            "activity frozen"
        } else {
            "local · private"
        })
        .alignment(ratatui::layout::Alignment::Right)
        .style(p.muted()),
        columns[1],
    );
}
pub(super) fn summary(frame: &mut Frame, app: &App, area: Rect, p: Palette) {
    let cards = Layout::horizontal([
        Constraint::Percentage(34),
        Constraint::Percentage(33),
        Constraint::Percentage(33),
    ])
    .spacing(1)
    .split(area);
    let total = app
        .snapshot
        .activity
        .iter()
        .fold((0u64, 0u64), |(a, b), process| {
            (
                a.saturating_add(process.rate_in),
                b.saturating_add(process.rate_out),
            )
        });
    let flows = app
        .snapshot
        .activity
        .iter()
        .map(|process| process.connections.len())
        .sum::<usize>();
    let values = [
        (
            "OBSERVED ACTIVITY",
            format!("{} apps · {} peers", app.snapshot.activity.len(), flows),
        ),
        (
            "PERMISSIONS",
            format!(
                "{}\n{}",
                count_label(app.snapshot.applications.len(), "app"),
                count_label(app.snapshot.network.rules.len(), "network rule")
            ),
        ),
    ];
    for (index, (title, text)) in values.iter().enumerate() {
        frame.render_widget(
            Paragraph::new(text.as_str())
                .block(p.block(*title))
                .style(Style::default().fg(p.text))
                .wrap(Wrap { trim: true }),
            cards[index],
        );
    }
    let block = p.block("TRANSFER");
    let inner = block.inner(cards[2]);
    frame.render_widget(block, cards[2]);
    if inner.width >= 24 {
        let col = Layout::horizontal([Constraint::Min(15), Constraint::Length(7)]).split(inner);
        frame.render_widget(
            Paragraph::new(format!("↓ {}/s\n↑ {}/s", bytes(total.0), bytes(total.1)))
                .style(p.muted()),
            col[0],
        );
        let rx: Vec<_> = app.chart.iter().map(|(a, _)| *a).collect();
        let tx: Vec<_> = app.chart.iter().map(|(_, b)| *b).collect();
        let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(col[1]);
        frame.render_widget(
            Sparkline::default()
                .data(&rx)
                .style(Style::default().fg(p.accent)),
            rows[0],
        );
        frame.render_widget(
            Sparkline::default()
                .data(&tx)
                .style(Style::default().fg(p.good)),
            rows[1],
        );
    } else {
        frame.render_widget(
            Paragraph::new(format!("↓ {}/s\n↑ {}/s", bytes(total.0), bytes(total.1)))
                .style(p.muted()),
            inner,
        );
    }
}
fn count_label(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}
