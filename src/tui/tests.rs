use super::*;
use rooklet_core::process::{SignalFailure, TerminationReport};
fn update(result: Result<Snapshot>, partial: bool) -> Update {
    Update {
        result,
        operation_error: None,
        kind: UpdateKind::Terminate,
        profile: None,
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
        firewall: Some(rooklet_core::model::FirewallSettings::default()),
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

#[test]
fn profile_result_preserves_completion_when_refresh_fails() {
    let mut app = App::new(Snapshot {
        firewall: Some(rooklet_core::model::FirewallSettings::default()),
        ..Default::default()
    });
    app.busy = true;
    let chart_length = app.chart.len();
    apply_update(
        &mut app,
        Update {
            result: Err(anyhow::anyhow!("observation unavailable")),
            operation_error: None,
            kind: UpdateKind::Profile,
            termination: None,
            profile: Some(rooklet::app::ProfileOutcome::Applied),
        },
        false,
    )
    .unwrap();
    assert!(!app.busy);
    assert!(app.snapshot.firewall.is_none());
    assert!(app.stale());
    assert_eq!(app.chart.len(), chart_length);
    let notice = app.notice.unwrap();
    assert!(notice.error);
    assert!(notice.text.contains("applied"));
    assert!(notice.text.contains("observation unavailable"));
}

#[test]
fn partial_mutation_reports_failure_with_verified_current_state() {
    let mut app = App::new(Snapshot::default());
    app.busy = true;
    let snapshot = Snapshot {
        firewall: Some(rooklet_core::model::FirewallSettings {
            stealth: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    apply_update(
        &mut app,
        Update {
            result: Ok(snapshot),
            operation_error: Some(anyhow::anyhow!("partial failure")),
            kind: UpdateKind::Firewall,
            termination: None,
            profile: None,
        },
        false,
    )
    .unwrap();
    assert!(app.snapshot.firewall.unwrap().stealth);
    assert!(!app.busy);
    assert!(app.notice.unwrap().error);
}

#[test]
fn partial_mutation_and_failed_refresh_invalidate_all_cached_scopes() {
    let mut app = App::new(Snapshot {
        firewall: Some(Default::default()),
        applications_available: true,
        network: rooklet_core::model::NetworkStatus {
            rules_available: true,
            configured: true,
            applied: true,
            ..Default::default()
        },
        ..Default::default()
    });
    app.busy = true;
    apply_update(
        &mut app,
        Update {
            result: Err(anyhow::anyhow!("refresh unavailable")),
            operation_error: Some(anyhow::anyhow!("partial failure")),
            kind: UpdateKind::Firewall,
            termination: None,
            profile: None,
        },
        false,
    )
    .unwrap();
    assert!(!app.busy);
    assert!(app.stale());
    assert!(app.snapshot.firewall.is_none());
    assert!(!app.snapshot.applications_available);
    assert!(!app.snapshot.network.rules_available);
    assert!(!app.snapshot.network.applied);
    assert!(
        app.notice
            .unwrap()
            .text
            .contains("partial failure; status refresh failed: refresh unavailable")
    );
}

#[test]
fn backend_disconnection_exits_both_active_and_closing_sessions() {
    for closing in [false, true] {
        let mut app = App::new(Snapshot::default());
        app.busy = true;
        let error = apply_worker_updates(
            &mut app,
            Drained {
                updates: Vec::new(),
                failure: Some(anyhow::anyhow!(
                    "backend worker stopped; the pending operation outcome is unknown"
                )),
            },
            closing,
        )
        .unwrap_err();
        assert!(error.to_string().contains("outcome is unknown"));
        assert!(!app.busy);
        assert!(app.stale());
        assert!(app.notice.unwrap().error);
    }
}

#[test]
fn backend_disconnection_preserves_queued_completion_before_exit() {
    for closing in [false, true] {
        let mut app = App::new(Snapshot::default());
        let effect = app.handle(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('p'),
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(effect.profile.is_some());
        let error = apply_worker_updates(
            &mut app,
            Drained {
                updates: vec![Update {
                    result: Ok(Snapshot::default()),
                    operation_error: None,
                    kind: UpdateKind::Profile,
                    termination: None,
                    profile: Some(rooklet::app::ProfileOutcome::Listed(vec!["Home".into()])),
                }],
                failure: Some(anyhow::anyhow!("backend worker stopped")),
            },
            closing,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "backend worker stopped");
        assert!(!app.busy);
        assert!(matches!(
            app.popup,
            Some(rooklet::app::Popup::Profiles { entries, loading: false, .. })
                if entries == ["Home"]
        ));
    }
}
