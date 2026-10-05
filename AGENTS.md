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

## Workspace and code map

The workspace has one executable and two internal libraries. Shared dependency
versions, edition, MSRV, license, and Rust lints live in the root Cargo.toml.

- `crates/rooklet-core/src/`: platform-free models and observation/termination types,
  captured permission evidence, pure schemas/path validation, PF compilation and
  rule analysis, and terminal text sanitization.
- `crates/rooklet-macos/src/`: incoming firewall/PF operations, profile preparation,
  storage and transactions, GeoIP, filesystem evidence, and bounded subprocess,
  traffic, process identity/signaling, and resource collection implementations.
- `src/main.rs`: minimal binary entry point.
- `src/cli/`: arguments, dispatch, bounded input, and profile commands.
- `src/auth.rs` and `src/json.rs`: terminal authentication and CLI output.
- `src/tui.rs` and `src/tui/`: terminal lifecycle, events, and bounded workers.
- `src/app.rs` and `src/app/`: interaction state, queries, selection, and dialogs.
- `src/ui.rs` and `src/ui/`: view composition, themes, modal rendering, and mouse geometry.
- `src/presentation.rs`: application-specific traffic formatting.
- `tests/`: CLI, interaction, distribution, rendering, and read-only PF syntax regressions.
- `crates/*/tests/` and private module tests: domain and platform behavior regressions.
  GeoIP fixtures and their provenance/licenses live in `crates/rooklet-macos/tests/data/`.
- `tests/visual_preview.rs` and `scripts/render-preview.py`: actual terminal-cell previews.
- `scripts/build-release.sh` and `scripts/package.sh`: explicit macOS executable
  target builds and verified binary-only archives.
- `scripts/install.sh` and `scripts/uninstall.sh`: binary-only installation/removal;
  never change live rules or bypass quarantine.

Dependencies flow from the application to both libraries, and from macOS to core.
Core must not depend on Clap, Ratatui, native APIs, filesystem queries, subprocesses,
or either other package. Import shared types directly from their owning crate;
do not restore former application module paths through forwarding re-exports.

Keep facades focused on their public operations. Parsers, subprocess execution,
native collection, and transaction internals remain private. Keep captured profile
baselines opaque. Split modules by cohesion rather than arbitrary line counts;
avoid catch-all utility modules and a crate for every backend.

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
- Resource collection stays outside rendering, uses verified PID generations, and remains
  bounded by time, process count, and cache age. CPU 100% represents one core; memory is an
  estimated footprint. Keep missing, warming, partial, stale, and disabled readings explicit.
- Routine cached firewall observations must never replace forced reads for mutations,
  profile preparation, or explicit refresh. Resource sampling must not delay accepted transactions.
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
cargo fmt --all --check
cargo check --workspace --locked --all-targets
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo test --workspace --locked
cargo build --release --locked --package rooklet --bin rooklet
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
