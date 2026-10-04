use std::{net::IpAddr, time::Duration};
use xield::{
    activity::{Tracker, parse_csv, parse_flow},
    geoip::{GeoIp, is_local},
    model::Protocol,
};
fn sample(incoming: u64, outgoing: u64, flow_in: u64, flow_out: u64) -> String {
    format!(
        ",bytes_in,bytes_out,\n\"Example, App.987654\",{incoming},{outgoing},\ntcp4 192.168.1.2:1234<->8.8.8.8:443,{flow_in},{flow_out},\ntcp6 fe80::1%en0.5555<->fe80::2%en0.6666,10,20,\nudp4 *:5353<->*:*,100,200,\ntcp4 *:22<->*:*,,,\n"
    )
}
#[test]
fn csv_handles_quoted_names_numeric_counters_and_scoped_ipv6() {
    let samples = parse_csv(&sample(2000, 1000, 1200, 600)).unwrap();
    let process = &samples[0][0];
    assert_eq!(process.name, "Example, App");
    assert_eq!(process.pid, 987654);
    assert_eq!(process.bytes_in, 2000);
    assert_eq!(process.flows.len(), 2);
    assert_eq!(process.flows[0].remote.to_string(), "8.8.8.8");
    assert_eq!(process.flows[0].port, Some(443));
    assert_eq!(process.flows[1].remote.to_string(), "fe80::2");
    assert_eq!(process.flows[1].port, Some(6666));
    assert!(parse_flow("udp6 ::1.33<->2001:4860:4860::8888.53").is_some());
    assert!(parse_flow("tcp6 [::1]:33<->[2001:4860:4860::8888]:443").is_some());
    assert_eq!(
        parse_flow("udp4 1.2.3.4:3<->8.8.8.8:53").unwrap().2,
        Protocol::Udp
    );
    assert!(parse_flow("tcp4 *:22<->*:*").is_none());
}
#[test]
fn header_boundaries_and_malformed_records_are_explicit() {
    assert_eq!(
        parse_csv(&(sample(1, 2, 1, 2) + &sample(2, 3, 2, 3)))
            .unwrap()
            .len(),
        2
    );
    for bad in [
        "App.4,1,2,",
        ",bytes_in,bytes_out,\nApp.bad,1,2,",
        ",bytes_in,bytes_out,\nApp.4,1k,2,",
        ",bytes_in,bytes_out,\ntcp4 1.1.1.1:2<->2.2.2.2:3,1,2,",
    ] {
        assert!(parse_csv(bad).is_err());
    }
}
#[test]
fn totals_start_at_observation_rates_use_elapsed_and_resets_do_not_spike() {
    let mut tracker = Tracker::default();
    let mut geoip = GeoIp::default();
    let ingest = |tracker: &mut Tracker,
                  geoip: &mut GeoIp,
                  incoming,
                  outgoing,
                  flow_in,
                  flow_out,
                  millis| {
        tracker.update(
            parse_csv(&sample(incoming, outgoing, flow_in, flow_out))
                .unwrap()
                .remove(0),
            Duration::from_millis(millis),
            geoip,
        )
    };
    let first = ingest(&mut tracker, &mut geoip, 2000, 1000, 1200, 600, 0);
    assert_eq!((first[0].bytes_in, first[0].rate_in), (0, 0));
    assert_eq!(first[0].connections[0].bytes_in, 0);
    let second = ingest(&mut tracker, &mut geoip, 3000, 1600, 1800, 1000, 2000);
    assert_eq!(
        (
            second[0].bytes_in,
            second[0].bytes_out,
            second[0].rate_in,
            second[0].rate_out
        ),
        (1000, 600, 500, 300)
    );
    assert_eq!(second[0].connections[0].bytes_in, 600); // No process + socket double-count.
    assert!(second[0].connections[0].country.is_none());
    assert!(second[0].connections[1].local);
    let reset = ingest(&mut tracker, &mut geoip, 10, 20, 3, 4, 3000);
    assert_eq!(
        (
            reset[0].bytes_in,
            reset[0].bytes_out,
            reset[0].rate_in,
            reset[0].rate_out
        ),
        (1000, 600, 0, 0)
    );
    let next = ingest(&mut tracker, &mut geoip, 110, 70, 13, 14, 4000);
    assert_eq!((next[0].bytes_in, next[0].rate_in), (1100, 100));
}
#[test]
fn local_and_missing_database_are_offline() {
    for address in [
        "127.0.0.1",
        "10.0.0.1",
        "169.254.1.1",
        "::1",
        "fe80::1",
        "fd00::1",
        "::ffff:192.168.1.1",
        "224.0.0.251",
    ] {
        assert!(is_local(address.parse::<IpAddr>().unwrap()), "{address}");
    }
    assert!(!is_local("8.8.8.8".parse().unwrap()));
    assert!(!is_local("2606:4700:4700::1111".parse().unwrap()));
    let mut geoip = GeoIp::default();
    assert!(geoip.description().is_none());
    assert!(geoip.lookup("8.8.8.8".parse().unwrap()).is_none());
}

#[test]
fn protocol_like_process_names_are_not_socket_rows() {
    let samples =
        parse_csv(",bytes_in,bytes_out,\ntcpdump.999999,12,34,\nudpservice.999998,56,78,\n")
            .unwrap();
    assert_eq!(samples[0].len(), 2);
    assert_eq!(samples[0][0].name, "tcpdump");
    assert_eq!(samples[0][1].name, "udpservice");
}
#[test]
fn duplicate_process_rows_do_not_double_count_and_reappearance_keeps_baselines() {
    let mut tracker = Tracker::default();
    let mut geoip = GeoIp::default();
    let baseline = ",bytes_in,bytes_out,\nExample.999999,100,50,\nExample.999999,100,50,\n";
    assert_eq!(
        tracker
            .update(
                parse_csv(baseline).unwrap().remove(0),
                Duration::ZERO,
                &mut geoip
            )
            .len(),
        1
    );
    assert!(
        tracker
            .update(Vec::new(), Duration::from_secs(1), &mut geoip)
            .is_empty()
    );
    let next = ",bytes_in,bytes_out,\nExample.999999,300,150,\nExample.999999,300,150,\n";
    let processes = tracker.update(
        parse_csv(next).unwrap().remove(0),
        Duration::from_secs(2),
        &mut geoip,
    );
    assert_eq!(processes.len(), 1);
    assert_eq!((processes[0].bytes_in, processes[0].rate_in), (200, 100));
}
#[test]
fn csv_record_size_is_bounded() {
    let line = format!(
        ",bytes_in,bytes_out,\n{}.999999,0,0,\n",
        "x".repeat(64 * 1024)
    );
    assert!(parse_csv(&line).is_err());
}

#[test]
fn app_group_session_totals_survive_helper_exit_and_counter_reset() {
    use xield::activity::RawProcess;
    let temp = tempfile::tempdir().unwrap();
    let bundle = temp.path().join("Example.app");
    std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
    std::fs::write(bundle.join("Contents/Info.plist"), b"<plist/>").unwrap();
    std::fs::write(bundle.join("Contents/MacOS/Example"), b"test executable").unwrap();
    let executable = bundle
        .join("Contents/MacOS/Example")
        .to_string_lossy()
        .into_owned();
    let raw = |pid, incoming, outgoing| RawProcess {
        pid,
        name: format!("worker-{pid}"),
        bytes_in: incoming,
        bytes_out: outgoing,
        flows: Vec::new(),
    };
    let mut tracker = Tracker::default();
    let mut geoip = GeoIp::default();
    let ingest = |tracker: &mut Tracker, geoip: &mut GeoIp, sample, seconds| {
        tracker.update_with_paths(sample, Duration::from_secs(seconds), geoip, |_| {
            Some(executable.clone())
        })
    };
    let first = ingest(
        &mut tracker,
        &mut geoip,
        vec![raw(101, 100, 100), raw(102, 200, 200)],
        0,
    );
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].bytes_in, 0);
    let second = ingest(
        &mut tracker,
        &mut geoip,
        vec![raw(101, 150, 120), raw(102, 300, 240)],
        1,
    );
    assert_eq!(
        (second[0].bytes_in, second[0].bytes_out, second[0].rate_in),
        (150, 60, 150)
    );
    let exited = ingest(&mut tracker, &mut geoip, vec![raw(101, 200, 140)], 2);
    assert_eq!(
        (exited[0].bytes_in, exited[0].bytes_out, exited[0].rate_in),
        (200, 80, 50)
    );
    let reset = ingest(&mut tracker, &mut geoip, vec![raw(101, 40, 10)], 3);
    assert_eq!(
        (reset[0].bytes_in, reset[0].bytes_out, reset[0].rate_in),
        (200, 80, 0)
    );
    let replacement = ingest(
        &mut tracker,
        &mut geoip,
        vec![raw(101, 90, 20), raw(103, 9000, 9000)],
        4,
    );
    assert_eq!(
        (
            replacement[0].bytes_in,
            replacement[0].bytes_out,
            replacement[0].rate_in
        ),
        (250, 90, 50)
    );
    assert_eq!(replacement[0].path.as_deref(), bundle.to_str());
    assert!(ingest(&mut tracker, &mut geoip, Vec::new(), 5).is_empty());
    let reappeared = ingest(&mut tracker, &mut geoip, vec![raw(104, 8000, 8000)], 70);
    assert_eq!(reappeared[0].bytes_in, 250);
}
#[test]
fn an_unverified_app_directory_does_not_group_unrelated_processes() {
    use xield::activity::RawProcess;
    let temp = tempfile::tempdir().unwrap();
    let bundle = temp.path().join("Unverified.app");
    std::fs::create_dir(&bundle).unwrap();
    let mut tracker = Tracker::default();
    let mut geoip = GeoIp::default();
    let sample = [10, 11]
        .into_iter()
        .map(|pid| RawProcess {
            pid,
            name: "same-name".into(),
            bytes_in: 0,
            bytes_out: 0,
            flows: Vec::new(),
        })
        .collect();
    let processes = tracker.update_with_paths(sample, Duration::ZERO, &mut geoip, |_| {
        Some(bundle.join("unverified").to_string_lossy().into_owned())
    });
    assert_eq!(processes.len(), 2);
}

#[test]
fn grouped_helpers_merge_same_peer_without_adding_flow_bytes_to_app_total() {
    use xield::activity::{RawFlow, RawProcess};
    let temp = tempfile::tempdir().unwrap();
    let bundle = temp.path().join("Example.app");
    std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
    std::fs::write(bundle.join("Contents/Info.plist"), b"<plist/>").unwrap();
    std::fs::write(bundle.join("Contents/MacOS/Example"), b"test executable").unwrap();
    let executable = bundle
        .join("Contents/MacOS/Example")
        .to_string_lossy()
        .into_owned();
    let raw = |pid, incoming, outgoing, flow_in, flow_out| RawProcess {
        pid,
        name: format!("worker-{pid}"),
        bytes_in: incoming,
        bytes_out: outgoing,
        flows: vec![RawFlow {
            key: format!("tcp4 192.168.1.1:{pid}<->8.8.8.8:443"),
            remote: "8.8.8.8".parse().unwrap(),
            port: Some(443),
            protocol: Protocol::Tcp,
            bytes_in: flow_in,
            bytes_out: flow_out,
        }],
    };
    let mut tracker = Tracker::default();
    let mut geoip = GeoIp::default();
    tracker.update_with_paths(
        vec![raw(101, 100, 100, 50, 20), raw(102, 200, 200, 75, 30)],
        Duration::ZERO,
        &mut geoip,
        |_| Some(executable.clone()),
    );
    let groups = tracker.update_with_paths(
        vec![raw(101, 150, 120, 80, 30), raw(102, 300, 240, 110, 40)],
        Duration::from_secs(1),
        &mut geoip,
        |_| Some(executable.clone()),
    );
    assert_eq!(groups.len(), 1);
    assert_eq!((groups[0].bytes_in, groups[0].bytes_out), (150, 60));
    assert_eq!(groups[0].connections.len(), 1);
    assert_eq!(
        (
            groups[0].connections[0].bytes_in,
            groups[0].connections[0].bytes_out
        ),
        (65, 20)
    );
}

#[test]
fn macos_quic_flows_use_udp_transport_and_preserve_destinations() {
    let samples = parse_csv(",bytes_in,bytes_out,\nquic-client.999999,150,80,\nquic4 192.168.99.196:50814<->35.190.80.1:443,100,50,\nquic6 fe80::1%en0.5555<->2606:4700:4700::1111.443,50,30,\n").unwrap();
    assert_eq!(samples[0][0].flows.len(), 2);
    assert!(
        samples[0][0]
            .flows
            .iter()
            .all(|flow| flow.protocol == Protocol::Udp)
    );
    assert_eq!(
        samples[0][0].flows[1].remote.to_string(),
        "2606:4700:4700::1111"
    );
}
