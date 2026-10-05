//! Explicit hypothetical packet fields, separate from observed peer metadata.
use crate::{
    app::{App, MouseAction, NetworkDraft, RuleProbe},
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
        ("Destination port", display_value(&draft.port, "unknown")),
        ("Protocol", draft.protocol.to_string()),
        ("Direction", draft.direction.to_string()),
        ("Interface", display_value(&draft.interface, "unknown")),
    ];
    let remainder = fields(frame, area, &values, field, 18, p, hits);
    let result = explanation(app, draft);
    frame.render_widget(
        Paragraph::new(format!(
            concat!(
                "Xield anchor prediction; not a live verdict.\n{}\n\n",
                "Port: local for inbound, remote for outbound. Blank fields stay unknown.\n",
                "Other anchors and existing PF states can affect traffic.\n",
                "Tab / ↑↓ fields · ←→ choices · type edits"
            ),
            result
        ))
        .style(p.muted())
        .wrap(Wrap { trim: true }),
        remainder,
    );
}

fn explanation(app: &App, draft: &RuleProbe) -> String {
    if !app.snapshot.network.rules_available {
        return "Saved rules unavailable; authenticate and refresh in Network.".into();
    }
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
}

pub(super) fn edit(
    frame: &mut Frame,
    draft: &NetworkDraft,
    field: usize,
    area: Rect,
    p: Palette,
    hits: &mut HitMap,
) {
    let values = [
        ("Remote IP/CIDR", draft.destination.clone()),
        ("Destination port", display_value(&draft.port, "any")),
        ("Protocol", draft.protocol.to_string()),
        ("Direction", draft.direction.to_string()),
        ("Action", draft.action.to_string()),
        ("Interface", display_value(&draft.interface, "any")),
        ("Name", draft.name.clone()),
    ];
    let remainder = fields(frame, area, &values, field, 19, p, hits);
    frame.render_widget(
        Paragraph::new(concat!(
            "\nTab / ↑↓ field · ←→ / Space changes choice\n",
            "Type edits text · Enter reviews · Esc cancels\n",
            "Direction is proposed policy, not observed direction.\n",
            "Applies to every app. Port is local for inbound traffic."
        ))
        .style(Style::default().fg(p.text)),
        remainder,
    );
}

fn display_value(value: &str, empty: &str) -> String {
    if value.is_empty() {
        empty.into()
    } else {
        value.into()
    }
}

fn fields(
    frame: &mut Frame,
    area: Rect,
    values: &[(&str, String)],
    field: usize,
    label_width: usize,
    p: Palette,
    hits: &mut HitMap,
) -> Rect {
    let height = area.height.min(values.len() as u16);
    for (index, (label, value)) in values.iter().take(height.into()).enumerate() {
        let row = Rect::new(area.x, area.y + index as u16, area.width, 1);
        let style = if field == index {
            Style::default().fg(p.accent).bg(p.selection)
        } else {
            Style::default().fg(p.text)
        };
        // One row per field keeps pointer targets aligned with long/clipped text.
        frame.render_widget(
            Paragraph::new(Line::raw(format!(
                "{} {label:<label_width$} {}",
                if field == index { "›" } else { " " },
                clean(value)
            )))
            .style(style),
            row,
        );
        hits.click(row, MouseAction::DialogField(index));
    }
    Rect::new(area.x, area.y + height, area.width, area.height - height)
}
