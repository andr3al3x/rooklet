# 0006: Resolve countries offline with explicit database updates

- Status: Accepted
- Recorded: 2026-10-05
- Scope: GeoIP privacy, storage, and network access
- Supersedes: None
- Superseded by: None

## Context and constraints

Country summaries must not disclose observed endpoint addresses to an external
lookup service. The application must remain useful without a database, credentials,
or network access. Database downloads are untrusted and have separate licensing.
This record describes the implemented managed GeoIP workflow.

## Decision

Use the unmodified DB-IP Country Lite MMDB for local lookups and a bounded cache.
Install/update only through explicit CLI or Settings actions using provider
HTTPS downloads. Bound archive/data/schema validation, then atomically replace the
managed current-user database. Preserve prior valid data on failed installation
or reload. Running sessions detect replacements without background downloads.

Render local/private addresses as `Local`, unavailable matches as `Unknown`, and
public country labels as estimates. Retain DB-IP attribution and licensing. Do not
bundle production country data or upload observed addresses.

## Alternatives

- Per-address remote lookup would expose monitoring data and introduce service
  availability, quotas, and request latency.
- Bundling the production database would tie freshness and licensed data delivery
  to every binary release.
- Automatic startup updates would add unrequested network activity and startup
  failure modes.

## Consequences

Lookups remain private and work offline after installation. Updates are explicit
and need no sudo or API key, but users must refresh data when wanted. Lite coverage
and IP-based geography are approximate; CDNs, VPNs, and proxies can mislead. The
database, validation bounds, cache lifecycle, and attribution need maintenance
independently of the binary release.

## Verification and references

- [GeoIP facade](../../crates/rooklet-macos/src/geoip.rs), [database validation/storage](../../crates/rooklet-macos/src/geoip/database.rs), [download workflow](../../crates/rooklet-macos/src/geoip/download.rs)
- [Offline tests](../../crates/rooklet-macos/src/geoip/offline_tests.rs), [update failure tests](../../crates/rooklet-macos/src/geoip/tests.rs)
- [Fixture provenance](../../crates/rooklet-macos/tests/data/README.md)
- [Country behavior](../functional.md#traffic-countries-and-resources)

Tests verify source validation, offline reads, bounded caching, safe replacement,
and preservation on failure. A database label does not verify an app's service
location or establish connection policy.
