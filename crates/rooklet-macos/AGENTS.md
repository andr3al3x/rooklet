# macOS crate guidance

This guide applies to `crates/rooklet-macos/`, including its tests. It supplements
the [root guidance](../../AGENTS.md) with platform operations and persistence requirements.

## Responsibilities

Module paths below are relative to this crate's `src/` directory.

- `backend.rs` and `backend/`: incoming firewall adapter, live observations, and control cache.
- `application.rs` and `permissions.rs`: executable validation, registration resolution,
  and filesystem evidence capture; pure matching stays in core.
- `network.rs` and `network/`: typed PF requests, trusted persistence, preflight, and lifecycle.
- `profile.rs` and `profile/`: opaque preparation baselines, review, storage, and transactions.
- Private `command`, `activity`, `process`, and `resources` modules: bounded execution,
  CSV/counter handling, native identity/signaling, and resource collection.
- `geoip.rs` and `geoip/`: offline lookup and explicit managed database updates.

## Privileges and lifecycle

- Expose typed operations; keep generic command execution and native helpers private.
  Use canonical self-executable paths for privileged helper requests.
- Never prompt for credentials or collect passwords. The application authenticates
  through normal sudo outside raw mode before calling privileged operations.
- Check cancellation before launch, bound tool output/deadlines, and reap children.
  Once an authorized PF transaction starts, finish it and any restoration even
  if the user quits; report supervision failures after the helper completes.
- Validate the entire proposed configuration before mutation and verify readback.
  ALF operations are sequential; report partial failures without claiming atomicity.
- Cached controls are for routine observation only. Mutations, profile preparation,
  and explicit refresh require fresh reads; failed readback invalidates cached controls.

## Firewall and process safety

- Preserve unrelated PF anchors, rules, states, and enable references. Never globally
  flush PF or disable it to remove Rooklet's rules; release only its owned reference.
- Keep persistence bounded, locked, and atomically replaced. Check root ownership,
  permissions, symlinks, managed configuration drift, and supported parent layouts.
- Keep profile baselines opaque. Reject changed reviews, preflight PF before changing
  incoming permissions, and report both application and restoration failures.
- Termination requires caller confirmation for the exact captured targets and signal.
  Recheck ownership, executable identity, start time, and kernel PID generation;
  protect root-owned processes, self, and ancestors, and reject stale requests.
  Never fall back to PID-only signaling or rediscover targets during termination.
- Successful signal delivery is not proof that a process exited.

## Observations and GeoIP

- Group helpers using verified paths. Preserve totals across helper exits; do not
  add process summaries and their child flows together. Keep rates separate from totals.
- Bound native collection by process count, cooperative time budget, and cache age.
  Keep discovery/counter work incremental and fair; distinguish individual native-call
  overruns from the cooperative budget. Collection must not starve accepted transactions.
- CPU 100% means one core. Memory is an estimated footprint; missing, warming, partial,
  stale, and disabled readings remain explicit. Reset rate baselines after identity
  changes, counter resets, failures, or resumed sampling.
- Keep native resource sampling costs distinct from the continuously running `nettop`
  traffic backend. Resource visibility does not control that subprocess's lifetime.
- Country lookups stay offline; never send observed endpoint addresses to a service.
  Install data only through explicit GeoIP update/Settings actions. Validate downloads
  before atomic replacement, preserve the old database on failure, and retain DB-IP
  license/attribution requirements. No background installation on startup.
- GeoIP test fixtures and provenance/licenses live in `tests/data/` within this crate.

## Diagnostic logging

Follow the [logging guide](../../docs/logging.md). The executable owns the
subscriber and sink; this crate emits explicit structured fields only. Use fixed
operation/tool/phase/outcome labels, counts, durations, availability/restoration
booleans, and numeric status or OS error codes. Never record raw errors,
arguments, paths, endpoints, process metadata, configurations, profiles, or
captured stdout/stderr at any level; do not derive fields with blanket
`#[instrument]` or `Debug`/`Display` of inputs.

Warn on availability transitions rather than every poll. Keep routine
observations at TRACE with aggregate timing and counts; do not emit individual
process or connection records. Preserve normal cancellation, transaction
completion/restoration, and child cleanup behavior while instrumenting them.

## Validation

Follow the root's safe testing limits. Use fake adapters for transaction failures,
drift, restoration, cancellation, races, counter resets, and bounded collection.
Native signal tests may exercise only their own spawned processes.

For focused macOS checks from the repository root:

```sh
cargo check -p rooklet-macos --locked --all-targets
cargo clippy -p rooklet-macos --locked --all-targets -- -D warnings
cargo test -p rooklet-macos --locked
```

Run root workspace checks before completing integrated code changes.
