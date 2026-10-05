//! Explicit hypothetical packet fields, separate from observed peer metadata.
use crate::{
    app::{App, MouseAction, RuleProbe},
    network::{self, QueryField, RuleExplanation},
    presentation::clean,
    ui::{HitMap, theme::Palette},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Paragraph, Wrap},
};

pub(super) fn explain(
    frame: &mut Frame,
    app: &App,
    draft: &RuleProbe,
    field: usize,
    area: Rect,
    p: Palette,
    hits: &mut HitMap,
) {
    let values = [
        ("Remote IP", draft.remote.clone()),
        (
            "Destination port",
            if draft.port.is_empty() {
                "unknown".into()
            } else {
                draft.port.clone()
            },
        ),
        ("Protocol", draft.protocol.to_string()),
        ("Direction", draft.direction.to_string()),
        (
            "Interface",
            if draft.interface.is_empty() {
                "unknown".into()
            } else {
                draft.interface.clone()
            },
        ),
    ];
    let field_height = area.height.min(values.len() as u16);
    for (index, (label, value)) in values.iter().take(field_height.into()).enumerate() {
        let row = Rect::new(area.x, area.y + index as u16, area.width, 1);
        frame.render_widget(
            Paragraph::new(Line::raw(format!(
                "{} {label:<18} {}",
                if field == index { "›" } else { " " },
                clean(value)
            )))
            .style(if field == index {
                Style::default().fg(p.accent).bg(p.selection)
            } else {
                p.style()
            }),
            row,
        );
        hits.click(row, MouseAction::DialogField(index));
    }
    let result = if !app.snapshot.network.rules_available {
        "Saved rules unavailable; authenticate and refresh in Network.".into()
    } else {
        match draft
            .query()
            .and_then(|query| network::explain_rules(&app.snapshot.network.rules, &query))
        {
            Ok(RuleExplanation::Matched {
                rule_id,
                position,
                action,
            }) => format!("First match: #{position} {} → {action}.", clean(&rule_id)),
            Ok(RuleExplanation::Indeterminate {
                rule_id,
                position,
                missing_fields,
            }) => {
                let fields = missing_fields
                    .iter()
                    .map(|field| match field {
                        QueryField::DestinationPort => "destination port",
                        QueryField::Interface => "interface",
                    })
                    .collect::<Vec<_>>()
                    .join(" and ");
                format!(
                    "Undetermined: #{position} {} might match. Supply {fields}.",
                    clean(&rule_id)
                )
            }
            Ok(RuleExplanation::NoMatch) => {
                "No enabled Xield rule matches; this does not imply allow.".into()
            }
            Err(error) => clean(&error.to_string()),
        }
    };
    let remainder = Rect::new(
        area.x,
        area.y + field_height,
        area.width,
        area.height - field_height,
    );
    frame.render_widget(Paragraph::new(format!("Xield anchor prediction; not a live verdict.\n{result}\n\nPort: local for inbound, remote for outbound. Blank fields stay unknown.\nOther anchors and existing PF states can affect traffic.\nTab / ↑↓ fields · ←→ choices · type edits"))
        .style(p.muted()).wrap(Wrap { trim: true }), remainder);
}
