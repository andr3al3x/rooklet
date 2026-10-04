//! Safe text and human-readable traffic formatting shared by views.
use crate::model::ProcessActivity;

pub fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
        })
        .collect()
}
pub fn bytes(value: u64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut number = value as f64;
    let mut unit = 0;
    while number >= 1024.0 && unit < units.len() - 1 {
        number /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value} B")
    } else {
        format!("{number:.1} {}", units[unit])
    }
}
pub fn countries(process: &ProcessActivity) -> String {
    let mut names: Vec<_> = process
        .connections
        .iter()
        .map(|flow| {
            if flow.local {
                "Local".into()
            } else {
                flow.country
                    .as_ref()
                    .map(|country| country.code.clone())
                    .unwrap_or_else(|| "Unknown".into())
            }
        })
        .collect();
    names.sort();
    names.dedup();
    if names.is_empty() {
        "Unknown".into()
    } else {
        names.join(", ")
    }
}
