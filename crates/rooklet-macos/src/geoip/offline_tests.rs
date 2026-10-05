use super::{GeoIp, is_local};
use std::net::IpAddr;

#[test]
fn empty_offline_database_never_invents_a_country() {
    let mut database = GeoIp::default();
    assert!(database.description().is_none());
    database.refresh().unwrap();
    for address in ["89.160.20.112", "2001:218::", "192.168.0.1", "1.1.1.1"] {
        assert!(
            database
                .lookup(address.parse::<IpAddr>().unwrap())
                .is_none()
        );
    }
}
#[test]
fn local_addresses_and_mapped_ipv4_stay_explicit() {
    for address in [
        "10.0.0.1",
        "127.0.0.1",
        "169.254.2.1",
        "0.0.0.0",
        "224.0.0.251",
        "255.255.255.255",
        "::1",
        "::",
        "fe80::1",
        "fc00::1",
        "ff02::1",
        "::ffff:192.168.1.1",
    ] {
        assert!(is_local(address.parse().unwrap()), "{address}");
    }
    for address in ["8.8.8.8", "2001:4860:4860::8888", "::ffff:8.8.8.8"] {
        assert!(!is_local(address.parse().unwrap()), "{address}");
    }
}
