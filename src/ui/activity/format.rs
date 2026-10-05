//! Compact, sanitized labels sized by terminal cells rather than UTF-8 bytes.
use crate::presentation::countries;
use ratatui::text::Line;
use rooklet_core::model::ProcessActivity;
use rooklet_core::text::clean;
use unicode_segmentation::UnicodeSegmentation;

pub(super) fn bytes(value: u64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut number = value as f64;
    let mut unit = 0;
    while number >= 1024.0 && unit < units.len() - 1 {
        number /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value}B")
    } else if (number * 10.0).round() >= 1000.0 || number.fract() == 0.0 {
        format!("{number:.0}{}", units[unit])
    } else {
        format!("{number:.1}{}", units[unit])
    }
}

pub(super) fn fit(text: &str, width: u16, keep_end: bool) -> String {
    let text = clean(text);
    if Line::raw(&text).width() <= width as usize {
        return text;
    }
    if width == 0 {
        return String::new();
    }
    let mut graphemes: Vec<_> = text.graphemes(true).collect();
    if keep_end {
        graphemes.reverse();
    }
    let mut used = 1;
    let mut kept = Vec::new();
    for grapheme in graphemes {
        let size = Line::raw(grapheme).width() as u16;
        if used + size > width {
            break;
        }
        kept.push(grapheme);
        used += size;
    }
    if keep_end {
        kept.reverse();
        format!("…{}", kept.concat())
    } else {
        format!("{}…", kept.concat())
    }
}

pub(super) fn country_summary(process: &ProcessActivity, width: u16) -> String {
    let labels = countries(process);
    let labels: Vec<_> = labels.split(", ").collect();
    let mut best = String::new();
    for count in 1..=labels.len() {
        let suffix = if count < labels.len() {
            format!(" +{}", labels.len() - count)
        } else {
            String::new()
        };
        let candidate = format!("{}{suffix}", labels[..count].join(", "));
        if Line::raw(&candidate).width() > width as usize {
            if best.is_empty() {
                let budget = width.saturating_sub(Line::raw(&suffix).width() as u16);
                best = format!("{}{suffix}", fit(labels[0], budget, false));
            }
            break;
        }
        best = candidate;
    }
    fit(&best, width, false)
}

/// Shorten at directory boundaries so an ellipsis never leaves half a component.
pub(super) fn path(text: &str, width: u16) -> String {
    let text = clean(text);
    if Line::raw(&text).width() <= width as usize {
        return text;
    }
    let mut components = text.rsplit('/').filter(|component| !component.is_empty());
    let Some(last) = components.next() else {
        return fit(&text, width, true);
    };
    let mut suffix = fit(last, width.saturating_sub(2), true);
    for component in components {
        let candidate = format!("{component}/{suffix}");
        if Line::raw(&candidate).width() + 2 > width as usize {
            break;
        }
        suffix = candidate;
    }
    fit(&format!("…/{suffix}"), width, true)
}
