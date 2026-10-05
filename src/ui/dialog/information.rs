//! Read-only help and observed-connection details.
use super::super::activity::country;
use crate::{
    app::{ActivityRow, App},
    presentation::bytes,
};
use rooklet_core::text::clean;

pub(super) fn help() -> Vec<String> {
    vec![
        "Tab / 1–4     Activity, Applications, Network, Settings".into(),
        "↑↓ / j k      Move selection".into(),
        "Enter         Expand, inspect, edit, or toggle".into(),
        "/             Search current view".into(),
        "Activity /    app: country: proto: incoming: scope: ip: port:".into(),
        "s / w         Sort Activity / explain Network rule matching".into(),
        "a / b         Allow / block INCOMING app connections".into(),
        "n             Add app path or machine-wide network rule".into(),
        "d / t / + -   Delete / toggle / reorder network rule".into(),
        "Space         Freeze activity display".into(),
        "r / i         Toggle Activity resources / inspect selected row".into(),
        "↑↓ / PgUp/Dn  Scroll process details".into(),
        "x / X         Terminate / force kill Activity app + helpers".into(),
        "Esc           Cancel dialog or clear search".into(),
        "u             Authenticate for PF status and changes".into(),
        "p             Profiles; Enter reviews, e exports current scopes".into(),
        "g in Settings Update the offline country database".into(),
        "q / Ctrl-C    Quit; applied firewall rules remain".into(),
        "Click selects · double-click opens · wheel moves".into(),
    ]
}

pub(super) fn inspect(app: &App, key: &str) -> Vec<String> {
    if let Some(process) = app
        .snapshot
        .activity
        .iter()
        .find(|process| crate::app::process_key(process) == key)
    {
        return process_details(app, process);
    }
    let peer = app.snapshot.activity.iter().find_map(|process| {
        process
            .connections
            .iter()
            .find(|flow| ActivityRow::Connection(process, flow).key() == key)
            .map(|flow| (process, flow))
    });
    if let Some((process, flow)) = peer {
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

fn state_label(state: rooklet_core::resources::ReadingState) -> &'static str {
    use rooklet_core::resources::ReadingState;
    match state {
        ReadingState::Fresh => "fresh",
        ReadingState::WarmingUp => "warming up",
        ReadingState::Stale => "stale",
        ReadingState::Unavailable => "unavailable",
        ReadingState::Partial => "partial",
    }
}

fn cpu(value: Option<f64>) -> String {
    value
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| format!("{value:.1}%"))
        .unwrap_or_else(|| "—".into())
}
fn memory(value: Option<u64>) -> String {
    value.map(bytes).unwrap_or_else(|| "—".into())
}
fn disk(value: Option<u64>) -> String {
    value
        .map(|value| format!("{}/s", bytes(value)))
        .unwrap_or_else(|| "—".into())
}

fn process_details(app: &App, process: &rooklet_core::model::ProcessActivity) -> Vec<String> {
    let resources = &app.snapshot.resources;
    let usage = resources.for_activity(process);
    let mut lines = vec![
        clean(&process.name),
        format!(
            "Path: {}",
            process
                .path
                .as_deref()
                .map(clean)
                .unwrap_or_else(|| "unresolved".into())
        ),
        format!(
            "Processes: {} captured",
            usage
                .map(|usage| usage.process_count)
                .unwrap_or(process.identities.len())
        ),
        "CPU 100% = one core; memory is an estimated footprint.".into(),
        "Disk rates are observed process I/O, separate from network traffic.".into(),
    ];
    if !app.resources_visible {
        lines.push("Resources disabled (r); enable to collect process readings.".into());
    }
    if !resources.enabled {
        lines.push("Resource sampling inactive; process readings are unavailable.".into());
    }
    if let Some(message) = &resources.message {
        lines.push(clean(message));
    }
    if let Some(usage) = usage {
        lines.push(format!(
            "Resources: {} · sampled {}/{} · age {}",
            state_label(usage.state),
            usage.sampled_count,
            usage.process_count,
            age(usage.age_ms)
        ));
        lines.push(format!(
            "CPU: {} · memory est: {}",
            cpu(usage.cpu_percent),
            memory(usage.memory_bytes)
        ));
        lines.push(format!(
            "Disk read: {} · write: {}",
            disk(usage.read_per_sec),
            disk(usage.write_per_sec)
        ));
    } else {
        lines.push("Resources unavailable (—); waiting for verified process samples.".into());
    }
    lines.push("Partial totals omit unavailable processes; stale readings are cached.".into());
    let identities = usage
        .map(|usage| {
            usage
                .processes
                .iter()
                .map(|reading| &reading.identity)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| process.identities.iter().collect());
    // Process capture is bounded; additionally cap modal content independently.
    for identity in identities.iter().take(64).copied() {
        lines.push(String::new());
        lines.push(format!(
            "PID {} · owner UID {} · parent PID {}",
            identity.pid, identity.uid, identity.parent_pid
        ));
        lines.push(format!(
            "Started: {}.{:06} Unix seconds",
            identity.start_sec, identity.start_usec
        ));
        lines.push(format!("Executable: {}", clean(&identity.path)));
        let reading = usage.and_then(|usage| {
            usage
                .processes
                .iter()
                .find(|reading| reading.identity == *identity)
        });
        if let Some(reading) = reading {
            lines.push(format!(
                "{} · age {} · CPU {} · memory est {}",
                state_label(reading.state),
                age(reading.age_ms),
                cpu(reading.cpu_percent),
                memory(reading.memory_bytes)
            ));
            lines.push(format!(
                "Disk read {} · write {}",
                disk(reading.read_per_sec),
                disk(reading.write_per_sec)
            ));
        } else {
            lines.push("CPU — · memory est — · disk — (unavailable)".into());
        }
    }
    if identities.len() > 64 {
        lines.push(format!(
            "{} additional captured processes omitted",
            identities.len() - 64
        ));
    }
    lines.push("Scroll ↑↓ / PgUp/PgDn; Esc closes.".into());
    lines
}

fn age(value: Option<u64>) -> String {
    value
        .map(|value| format!("{value} ms"))
        .unwrap_or_else(|| "unknown".into())
}
