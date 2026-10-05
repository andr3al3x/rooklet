//! Full-width observed app metrics with expandable peer details.
mod format;
use super::{HitMap, theme::Palette};
use crate::app::{ActivityRow, App, View, process_key};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Cell, Paragraph, Row, Table, TableState, Wrap},
};
use rooklet_core::model::{Connection, ProcessActivity};
use rooklet_core::text::clean;

#[derive(Clone, Copy)]
enum Column {
    Application,
    Peers,
    Country,
    Download,
    Upload,
    Received,
    Sent,
    Incoming,
    Path,
    Processes,
    Cpu,
    Memory,
}
impl Column {
    fn title(self) -> &'static str {
        match self {
            Self::Application => "APP / PEER",
            Self::Peers => "Peers",
            Self::Country => "Country",
            Self::Download => "↓/s",
            Self::Upload => "↑/s",
            Self::Received => "↓ Total",
            Self::Sent => "↑ Total",
            Self::Incoming => "Incoming",
            Self::Path => "Path",
            Self::Processes => "Procs",
            Self::Cpu => "CPU %",
            Self::Memory => "Mem est",
        }
    }
    fn numeric(self) -> bool {
        matches!(
            self,
            Self::Peers
                | Self::Download
                | Self::Upload
                | Self::Received
                | Self::Sent
                | Self::Processes
                | Self::Cpu
                | Self::Memory
        )
    }
}
fn columns(width: u16, resources: bool) -> Vec<(Column, u16)> {
    let mut columns = vec![(Column::Application, 0)];
    if width >= 76 {
        columns.push((Column::Peers, 5));
    }
    columns.extend([
        (Column::Country, if width >= 76 { 9 } else { 7 }),
        (Column::Download, 9),
        (Column::Upload, 9),
    ]);
    if width >= 76 {
        columns.extend([(Column::Received, 7), (Column::Sent, 7)]);
    }
    if width >= 110 {
        columns.push((Column::Incoming, 8));
        if resources && width >= 116 {
            columns.extend([
                (Column::Processes, 5),
                (Column::Cpu, 7),
                (Column::Memory, 9),
            ]);
        } else {
            columns.push((Column::Path, if width >= 130 { 28 } else { 20 }));
        }
    }
    // Borders and the selection marker each consume two cells; spacing consumes one per gap.
    let fixed = columns.iter().map(|(_, width)| *width).sum::<u16>() + columns.len() as u16 - 1 + 4;
    columns[0].1 = width.saturating_sub(fixed);
    columns
}

pub(super) fn draw(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    p: Palette,
    hits: &mut HitMap,
    state: &mut TableState,
) {
    let title = format!("OBSERVED TRAFFIC · Sort: {}", app.activity_sort.label());
    let mut block = p.block(title.clone());
    hits.click(
        Rect::new(
            area.x + 1,
            area.y,
            (Line::raw(title).width() as u16 + 2).min(area.width.saturating_sub(2)),
            1,
        ),
        crate::app::MouseAction::Key(crossterm::event::KeyCode::Char('s')),
    );
    let legend = if app.resources_visible && area.width >= 116 {
        " CPU 100% = 1 core · Mem estimated · ~ partial/stale · — warming/unknown · i details "
    } else if area.width >= 110 {
        " Totals since monitoring started · Peers / protocol · Incoming = registered app entries "
    } else if area.width >= 76 {
        " Totals since monitoring started · Peers / protocol "
    } else {
        " Totals since monitoring started "
    };
    block = block.title_bottom(Line::from(legend).style(p.muted()));
    let rows = app.activity_rows();
    if rows.is_empty() {
        let error = app.activity_filter_error();
        let reason = if let Some(error) = &error {
            error.as_str()
        } else if !app.filter().is_empty() {
            "No apps or peers match this search."
        } else {
            app.snapshot
                .notices
                .iter()
                .find(|notice| {
                    notice.to_lowercase().contains("activity")
                        || notice.to_lowercase().contains("nettop")
                })
                .map(String::as_str)
                .unwrap_or("Waiting for process statistics. Observed connections will appear here.")
        };
        frame.render_widget(
            Paragraph::new(clean(reason))
                .block(block)
                .wrap(Wrap { trim: true })
                .style(p.muted()),
            area,
        );
        return;
    }
    let columns = columns(area.width, app.resources_visible);
    let permissions = if area.width >= 110 {
        rooklet_core::permissions::Index::new(&app.snapshot)
    } else {
        rooklet_core::permissions::Index::default()
    };
    let data: Vec<_> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let expanded = match row {
                ActivityRow::Process(process) => {
                    app.expanded.contains(&process_key(process))
                        || matches!(rows.get(index + 1), Some(ActivityRow::Connection(_, _)))
                }
                ActivityRow::Connection(_, _) => false,
            };
            Row::new(
                columns
                    .iter()
                    .map(|&(column, width)| {
                        let (text, style) = match row {
                            ActivityRow::Process(process) => process_cell(
                                expanded,
                                &permissions,
                                app.snapshot
                                    .resources
                                    .for_activity(process)
                                    .filter(|_| app.snapshot.resources.enabled),
                                process,
                                column,
                                width,
                                p,
                            ),
                            ActivityRow::Connection(_, flow) => {
                                peer_cell(flow, column, width, area.width < 76, p)
                            }
                        };
                        let alignment = if column.numeric() {
                            Alignment::Right
                        } else {
                            Alignment::Left
                        };
                        Cell::from(Line::from(text).alignment(alignment)).style(style)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let table = Table::new(
        data,
        columns.iter().map(|(_, width)| Constraint::Length(*width)),
    )
    .header(
        Row::new(columns.iter().map(|(column, _)| column.title()))
            .style(p.muted())
            .bottom_margin(1),
    )
    .block(block)
    .column_spacing(1)
    .row_highlight_style(
        Style::default()
            .bg(p.selection)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("› ");
    frame.render_stateful_widget(table, area, &mut *state);
    let keys = rows.iter().map(ActivityRow::key).collect::<Vec<_>>();
    hits.table(area, 2, state.offset(), View::Activity, &keys);
}
fn process_cell(
    expanded: bool,
    permissions: &rooklet_core::permissions::Index,
    usage: Option<&rooklet_core::resources::Usage>,
    process: &ProcessActivity,
    column: Column,
    width: u16,
    p: Palette,
) -> (String, Style) {
    let text = match column {
        Column::Application => format!(
            "{} {}",
            if expanded { "▾" } else { "▸" },
            clean(&process.name)
        ),
        Column::Peers => process.connections.len().to_string(),
        Column::Country => format::country_summary(process, width),
        Column::Download => format!("{}/s", format::bytes(process.rate_in)),
        Column::Upload => format!("{}/s", format::bytes(process.rate_out)),
        Column::Received => format::bytes(process.bytes_in),
        Column::Sent => format::bytes(process.bytes_out),
        Column::Incoming => permissions.activity(process).state.label().into(),
        Column::Processes => usage
            .map(|usage| {
                if matches!(
                    usage.state,
                    rooklet_core::resources::ReadingState::Partial
                        | rooklet_core::resources::ReadingState::Stale
                ) {
                    format!("~{}", usage.process_count)
                } else {
                    usage.process_count.to_string()
                }
            })
            .unwrap_or_else(|| {
                if process.identities.is_empty() {
                    "—".into()
                } else {
                    process.identities.len().to_string()
                }
            }),
        Column::Cpu => resource_value(usage, Column::Cpu, |usage| {
            usage
                .cpu_percent
                .filter(|value| value.is_finite() && *value >= 0.0)
                .map(|value| format!("{value:.1}%"))
        }),
        Column::Memory => resource_value(usage, Column::Memory, |usage| {
            usage.memory_bytes.map(format::bytes)
        }),
        Column::Path => process
            .path
            .as_deref()
            .map(|path| format::path(path, width))
            .unwrap_or_else(|| "Unresolved".into()),
    };
    let style = match column {
        Column::Download => Style::default().fg(p.accent),
        Column::Upload => Style::default().fg(p.good),
        Column::Country | Column::Peers | Column::Path | Column::Processes => p.muted(),
        Column::Cpu | Column::Memory if text.starts_with('~') => Style::default().fg(p.warn),
        Column::Incoming => Style::default().fg(match text.as_str() {
            "Allow" => p.good,
            "Block" => p.bad,
            "Mixed" => p.warn,
            _ => p.muted,
        }),
        _ => Style::default().fg(p.text),
    };
    (
        format::fit(&text, width, matches!(column, Column::Path)),
        style,
    )
}
fn peer_cell(
    flow: &Connection,
    column: Column,
    width: u16,
    protocol_in_name: bool,
    p: Palette,
) -> (String, Style) {
    let text = match column {
        Column::Application => {
            let host = clean(&flow.remote_ip);
            let host = if host.contains(':') && flow.remote_port.is_some() {
                format!("[{host}]")
            } else {
                host
            };
            let port = flow
                .remote_port
                .map(|port| format!(":{port}"))
                .unwrap_or_default();
            if protocol_in_name {
                format!("  {} {host}{port}", flow.protocol)
            } else {
                format!("  {host}{port}")
            }
        }
        Column::Peers => flow.protocol.to_string(),
        Column::Country => {
            if flow.local {
                "Local".into()
            } else {
                flow.country
                    .as_ref()
                    .map(|country| clean(&country.code))
                    .unwrap_or_else(|| "Unknown".into())
            }
        }
        Column::Received => format::bytes(flow.bytes_in),
        Column::Sent => format::bytes(flow.bytes_out),
        _ => String::new(),
    };
    (format::fit(&text, width, false), p.muted())
}
pub(super) fn country(flow: &Connection) -> String {
    if flow.local {
        "Local network".into()
    } else {
        flow.country
            .as_ref()
            .map(|country| format!("{} · {}", clean(&country.code), clean(&country.name)))
            .unwrap_or_else(|| "Unknown".into())
    }
}

fn resource_value(
    usage: Option<&rooklet_core::resources::Usage>,
    column: Column,
    value: impl FnOnce(&rooklet_core::resources::Usage) -> Option<String>,
) -> String {
    let Some(usage) = usage else {
        return "—".into();
    };
    let Some(value) = value(usage) else {
        return "—".into();
    };
    match usage.state {
        rooklet_core::resources::ReadingState::Fresh => value,
        rooklet_core::resources::ReadingState::WarmingUp if matches!(column, Column::Memory) => {
            value
        }
        rooklet_core::resources::ReadingState::Partial
        | rooklet_core::resources::ReadingState::Stale => {
            format!("~{value}")
        }
        rooklet_core::resources::ReadingState::WarmingUp
        | rooklet_core::resources::ReadingState::Unavailable => "—".into(),
    }
}
