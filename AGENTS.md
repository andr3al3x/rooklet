# Repository guidance

## Product and scope

Rooklet is a standalone Rust terminal application for macOS firewall management and
local network activity monitoring. Keep the application slim, keyboard-first, and
clear about what each backend actually controls.

- `socketfilterfw` controls incoming application permissions and incoming firewall settings.
- PF controls machine-wide IP/CIDR, destination-port, protocol, direction, and interface rules.
- `nettop` supplies observed process and peer traffic; observation is not a firewall verdict.
- Country labels are estimates from the managed DB-IP Country Lite MMDB. Local or unknown addresses stay explicit.

This is a clean break from the former extension architecture. Do not introduce
Swift code, app bundles, system extensions, signing/provisioning workflows, RPC
compatibility layers, legacy CLI aliases, or old-schema migrations.

## Working approach

- For planning, review, or diagnosis, inspect and report; implement only when requested.
- Complete authorized work, make reasonable routine decisions, and preserve unrelated user changes.
- Inspect relevant code before editing. Use targeted `rg` searches and batch independent reads.
- Keep changes focused and idiomatic. Prefer explicit types and small functions over speculative abstractions.
- For substantial independent work, delegate with distinct file ownership or a read-only scope.
  Agents share the workspace; coordinate edits and validate the integrated result.
- Communicate meaningful findings and finish with actual validation and remaining limitations.

## Code map

- `src/model.rs`: typed shared data and mutations; strict configuration schemas.
- `src/main.rs`: minimal binary entry point.
- `src/cli/`: argument definitions, command dispatch, bounded configuration input, and profile commands.
- `src/auth.rs` and `src/json.rs`: terminal authentication and CLI output.
- `src/tui.rs` and `src/tui/`: event routing, terminal lifecycle, and bounded backend worker coordination.
- `src/app.rs` and `src/app/`: interaction state, selection, filtering, and typed dialogs.
- `src/ui.rs` and `src/ui/`: view composition, individual views, themes, and modal rendering.
- `src/profile.rs` and `src/profile/`: shared strict profile preparation, scope review,
  managed storage, drift checks, and verified application/restoration.
- `src/presentation.rs`: sanitized display text and traffic formatting.
- `src/permissions.rs`: verified Activity grouping to exact incoming registrations.
- `src/backend.rs` and `src/backend/`: backend facade, incoming firewall adapter/parsers,
  application path validation and live observations.
- `src/command.rs`: bounded subprocess execution, cancellation, and cleanup.
- `src/activity.rs` and `src/activity/`: observation facade, CSV parser, counters/grouping,
  process identity lookup, and monitor lifecycle.
- `src/process.rs` and `src/process/`: bounded process identity capture, verified app grouping, and confirmed identity-bound signaling.
- `src/geoip.rs` and `src/geoip/`: offline lookup, bounded country cache, and explicit managed database updates.
- `src/network.rs` and `src/network/`: PF facade, pure compiler/configuration checks,
  trusted persistence, subprocess adapter, and lifecycle transactions.
- `src/network/analysis.rs`: pure hypothetical rule matching and conservative single-rule shadowing.
- `tests/`: behavior and regression checks; fixtures must retain their provenance and licenses.
- `tests/visual_preview.rs` and `scripts/render-preview.py`: actual terminal-cell visual previews.
- `scripts/build-release.sh` and `scripts/package.sh`: explicit macOS target builds and verified archives.
- `scripts/install.sh` and `scripts/uninstall.sh`: binary-only installation/removal; never change live rules or bypass quarantine.

Keep facade files focused on composition and their public API. Put substantial
parsing, rendering, persistence, and transaction logic in modules for those
responsibilities. Split by cohesion rather than arbitrary line counts; avoid
catch-all utility modules. Keep implementation modules private and expose only
the operations their callers need.

## Reliability and security

- Invoke fixed executable paths with argument arrays. Never interpolate user input into shell commands.
- Use normal `sudo` authentication outside terminal raw mode. Never collect or store passwords.
  Worker subprocesses must not prompt for credentials.
- Bound configuration sizes, subprocess output, queues, history, and caches. Reap children and
  release resources on errors and shutdown.
- Validate the entire proposed configuration before mutation. Read back settings after changes.
  Report partial failures and failed restoration explicitly; do not claim atomicity across ALF operations.
- Preserve unrelated PF rules, anchors, states, and enable references. Never globally flush PF
  or disable it to remove Rooklet's rules. Respect root ownership, symlink checks, and configuration drift.
- Do not cancel an authorized PF transaction halfway through when the user quits the TUI.
- Process termination requires a confirmation for the exact captured targets and signal.
  Recheck ownership and identity, protect root-owned processes, self and ancestors,
  reject stale requests, and bind signals to the kernel PID generation. Never
  fall back to PID-only signaling. Test only processes created by the test itself.
- Treat command output, database content, paths, and process metadata as untrusted data.
  Sanitize terminal controls and bidi overrides before rendering.
- Keep totals and rates distinct, avoid counting both process summaries and their flows, and
  preserve app totals across helper exits. Process grouping must be supported by actual paths.
- Distinguish unavailable, inactive, stale, and observed state. Never fabricate observations
  or claim enforcement from a successfully loaded ruleset alone.
- Country lookups stay offline. Database updates, if added, must follow the provider's license
  and must not send observed endpoint addresses to an external service.
- Install GeoIP data only through explicit `geoip update` / Settings actions. Validate
  downloads before atomic replacement, retain the previous database on failure,
  display DB-IP attribution.

## Validation

Run from the repository root:

```sh
cargo fmt --check
cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

Fix compiler and Clippy warnings at their cause. Do not add blanket warning
suppression or weaken checks to obtain a passing result.

Add tests for meaningful behavior and failure cases: real command-output formats,
malformed inputs, counter resets, grouping, rule scopes/order, drift, cancellation,
selection stability, confirmation, and rendering at multiple terminal sizes.
Avoid tests that merely repeat the implementation.

Tests and routine verification must not change the machine's live firewall.
Read-only system queries and `pfctl -n` syntax validation are appropriate. Live
privileged mutation or connectivity tests require an explicit user request for
that testing scope; use an isolated test environment and a concrete restoration
procedure. Do not treat a source-code implementation request as activation approval.

For UI changes, render the actual Ratatui cells and inspect the result. Keep the
README and CLI help synchronized with the final behavior. Report what passed,
what was only reviewed, and what still requires live integration testing.
Pointer targets must follow rendered geometry, including table offsets and
clipping. Dialogs must block underlying targets; mouse actions must reuse the
same confirmation and authentication flow as keyboard actions.
