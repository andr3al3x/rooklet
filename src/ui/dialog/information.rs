//! Read-only help and observed-connection details.
use super::super::activity::country;
use crate::{
    app::{ActivityRow, App},
    presentation::{bytes, clean},
};

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
        "x / X         Terminate / force kill Activity app + helpers".into(),
        "Esc           Cancel dialog or clear search".into(),
        "u             Authenticate for PF status and changes".into(),
        "g in Settings Update the offline country database".into(),
        "q / Ctrl-C    Quit; applied firewall rules remain".into(),
        "Click selects · double-click opens · wheel moves".into(),
    ]
}

pub(super) fn inspect(app: &App, key: &str) -> Vec<String> {
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
