use xield::{
    app::{ActivityRow, ActivitySort, App, process_key},
    backend::Backend,
    model::{Application, Connection, Country, ProcessActivity, Protocol},
};

fn peer(ip: &str, port: u16, protocol: Protocol, local: bool) -> Connection {
    Connection {
        remote_ip: ip.into(),
        remote_port: Some(port),
        protocol,
        bytes_in: 40,
        bytes_out: 20,
        country: (!local).then(|| Country {
            code: "US".into(),
            name: "United States".into(),
        }),
        local,
    }
}
fn process(pid: u32, name: &str, connections: Vec<Connection>) -> ProcessActivity {
    ProcessActivity {
        pid,
        name: name.into(),
        path: Some(format!("/nonexistent/{name}")),
        identities: Vec::new(),
        bytes_in: 900,
        bytes_out: 800,
        rate_in: 30,
        rate_out: 20,
        connections,
    }
}
fn app() -> App {
    let mut snapshot = Backend::new(true).unwrap().snapshot().unwrap();
    snapshot.activity = vec![
        process(
            20,
            "Web Browser",
            vec![
                peer("203.0.113.4", 443, Protocol::Tcp, false),
                peer("192.168.0.4", 53, Protocol::Udp, true),
            ],
        ),
        process(10, "Idle", vec![]),
    ];
    snapshot.applications_available = true;
    snapshot.applications = vec![Application {
        path: "/nonexistent/Web Browser".into(),
        name: "Browser".into(),
        blocked: false,
    }];
    App::new(snapshot)
}
fn rows(app: &App) -> Vec<String> {
    app.activity_rows().iter().map(ActivityRow::key).collect()
}

#[test]
fn compound_flow_predicates_must_match_one_peer_and_keep_parent_totals() {
    let mut app = app();
    app.filters[0] = "app:Browser proto:tcp port:53".into();
    assert!(rows(&app).is_empty());
    app.filters[0] =
        "app:Browser country:US proto:tcp ip:203.0.113.0/24 port:443 scope:public incoming:allow"
            .into();
    let visible = app.activity_rows();
    assert_eq!(visible.len(), 2);
    assert_eq!(visible[0].process().bytes_in, 900);
    assert!(
        matches!(&visible[1], ActivityRow::Connection(_, flow) if flow.remote_port == Some(443))
    );
}

#[test]
fn peerless_apps_match_app_and_incoming_but_never_flow_predicates() {
    let mut app = app();
    app.filters[0] = "app:Idle incoming:unregistered".into();
    assert_eq!(rows(&app), vec!["process:10"]);
    for predicate in [
        "proto:any",
        "scope:public",
        "country:unknown",
        "ip:203.0.113.4",
        "port:443",
    ] {
        app.filters[0] = format!("app:Idle {predicate}");
        assert!(rows(&app).is_empty(), "{predicate}");
    }
    app.snapshot.applications_available = false;
    app.filters[0] = "incoming:unavailable".into();
    assert_eq!(rows(&app).len(), 2);
    app.filters[0] = "incoming:allow".into();
    assert!(rows(&app).is_empty());
}

#[test]
fn country_scope_and_ipv6_queries_use_observed_fields() {
    let mut app = app();
    app.filters[0] = "country:\"United States\"".into();
    assert_eq!(rows(&app).len(), 2);
    app.filters[0] = "country:local proto:udp".into();
    assert_eq!(rows(&app).len(), 2);
    app.snapshot.activity[0].connections[0].country = None;
    app.filters[0] = "country:unknown scope:public".into();
    assert_eq!(rows(&app).len(), 2);
    app.snapshot.activity[0].connections[0].remote_ip = "2001:db8::7".into();
    for filter in ["ip:2001:db8::/32", "2001:db8::7"] {
        app.filters[0] = filter.into();
        assert!(app.activity_filter_error().is_none());
        assert_eq!(rows(&app).len(), 2);
    }
}

#[test]
fn malformed_typed_filters_report_error_without_showing_misleading_rows() {
    let mut app = app();
    for filter in [
        "typo:value",
        "proto:icmp",
        "incoming:unknown",
        "scope:internet",
        "port:0",
        "port:65536",
        "port:abc",
        "ip:nope",
        "ip:192.0.2.0/99",
        "app:",
        "country:\"United States",
    ] {
        app.filters[0] = filter.into();
        assert!(app.activity_filter_error().is_some(), "{filter}");
        assert!(rows(&app).is_empty(), "{filter}");
    }
    app.filters[0] = "x".repeat(257);
    assert!(app.activity_filter_error().is_some());
}

#[test]
fn ordinary_multiword_search_remains_a_substring_and_autoexpands_peer_matches() {
    let mut app = app();
    app.filters[0] = "Web Browser".into();
    assert_eq!(rows(&app), vec!["process:20"]);
    app.filters[0] = "United States".into();
    assert_eq!(rows(&app).len(), 2);
    app.filters[0] = "proto:tcp United States".into();
    assert_eq!(rows(&app).len(), 2);
}

#[test]
fn sorting_preserves_selection_across_snapshots_and_breaks_ties_by_identity() {
    let mut app = app();
    let selected = process_key(&app.snapshot.activity[0]);
    app.selection[0] = Some(selected.clone());
    assert_eq!(rows(&app), vec!["process:20", "process:10"]);
    app.activity_sort = ActivitySort::DownloadRate;
    assert_eq!(rows(&app), vec!["process:10", "process:20"]);
    assert_eq!(app.selected_index(), Some(1));
    let mut snapshot = app.snapshot.clone();
    snapshot.activity.reverse();
    app.update(snapshot, false);
    assert_eq!(app.selected_key(), Some(selected.as_str()));
    assert_eq!(rows(&app), vec!["process:10", "process:20"]);
    app.snapshot.activity[0].rate_in = 100;
    app.activity_sort = ActivitySort::Peers;
    assert_eq!(rows(&app), vec!["process:20", "process:10"]);
    app.activity_sort = ActivitySort::Name;
    assert_eq!(rows(&app), vec!["process:10", "process:20"]);
    app.activity_sort = ActivitySort::Snapshot;
    assert_eq!(rows(&app), vec!["process:10", "process:20"]);
}

#[test]
fn sort_cycle_covers_metrics_and_descending_numeric_order() {
    let mut app = app();
    app.snapshot.activity[1].rate_out = 200;
    app.snapshot.activity[1].bytes_in = 2000;
    app.snapshot.activity[1].bytes_out = 2000;
    for sort in [
        ActivitySort::UploadRate,
        ActivitySort::DownloadTotal,
        ActivitySort::UploadTotal,
    ] {
        app.activity_sort = sort;
        assert_eq!(rows(&app), vec!["process:10", "process:20"]);
    }
    let mut sort = ActivitySort::default();
    for _ in 0..7 {
        assert!(!sort.label().is_empty());
        sort = sort.next();
    }
    assert_eq!(sort, ActivitySort::default());
}

#[test]
fn incoming_filters_use_grouped_permission_resolution() {
    let mut snapshot = Backend::new(true).unwrap().snapshot().unwrap();
    snapshot.activity.truncate(1);
    snapshot.activity[0].connections.clear();
    let identity = snapshot.activity[0].identities[0].clone();
    let mut helper = identity.clone();
    helper.pid += 10;
    helper.path = format!(
        "{}/Contents/Helpers/helper",
        identity.bundle_path.as_deref().unwrap()
    );
    snapshot.activity[0].identities.push(helper.clone());
    snapshot.applications = vec![
        Application {
            path: identity.path,
            name: "main".into(),
            blocked: false,
        },
        Application {
            path: helper.path,
            name: "helper".into(),
            blocked: true,
        },
    ];
    let mut app = App::new(snapshot);
    app.filters[0] = "incoming:mixed".into();
    assert_eq!(rows(&app).len(), 1);
    app.filters[0] = "incoming:block".into();
    assert!(rows(&app).is_empty());
    for application in &mut app.snapshot.applications {
        application.blocked = true;
    }
    assert_eq!(rows(&app).len(), 1);
    app.snapshot.activity[0].path = None;
    app.filters[0] = "incoming:unavailable".into();
    assert_eq!(rows(&app).len(), 1);
    app.filters[0] = "incoming:unregistered".into();
    assert!(rows(&app).is_empty());
}
