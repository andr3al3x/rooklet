use super::*;
use xield::process::{SignalFailure, TerminationReport};
fn update(result: Result<Snapshot>, partial: bool) -> Update {
    Update {
        result,
        kind: UpdateKind::Terminate,
        termination: Some(TerminationReport {
            attempted: 2,
            delivered: vec![201],
            failures: if partial {
                vec![SignalFailure {
                    pid: 204,
                    reason: "process changed".into(),
                }]
            } else {
                Vec::new()
            },
        }),
    }
}
#[test]
fn quitting_surfaces_partial_termination_failures() {
    let mut app = App::new(Snapshot::default());
    let error = apply_update(&mut app, update(Ok(Snapshot::default()), true), true).unwrap_err();
    assert!(error.to_string().contains("1/2"));
    assert!(error.to_string().contains("process changed"));
}
#[test]
fn refresh_error_retains_signal_report_and_invalidates_cached_health() {
    let mut app = App::new(Snapshot {
        firewall: Some(xield::model::FirewallSettings::default()),
        ..Default::default()
    });
    app.busy = true;
    apply_update(
        &mut app,
        update(Err(anyhow::anyhow!("observation unavailable")), false),
        false,
    )
    .unwrap();
    assert!(!app.busy);
    assert!(app.snapshot.firewall.is_none());
    let notice = app.notice.unwrap();
    assert!(notice.error);
    assert!(notice.text.contains("Signal delivered"));
    assert!(notice.text.contains("observation unavailable"));
}
#[test]
fn quitting_surfaces_post_signal_observation_failure() {
    let mut app = App::new(Snapshot::default());
    let error = apply_update(
        &mut app,
        update(Err(anyhow::anyhow!("observation unavailable")), false),
        true,
    )
    .unwrap_err();
    assert!(error.to_string().contains("Signal delivered"));
    assert!(error.to_string().contains("observation unavailable"));
}
