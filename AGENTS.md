# Repository guidance

## Product and boundaries

Rooklet is a slim, keyboard-first Rust terminal application for macOS firewall
management and local activity monitoring, with mouse navigation.

- `socketfilterfw` controls incoming application permissions and settings.
- PF controls machine-wide network rules, not outgoing per-app permissions.
- `nettop` supplies sampled observations, not firewall verdicts or a complete
  log of blocked attempts. Country labels are offline DB-IP estimates.
- Keep delivery binary-only. Do not add Swift, app bundles, system extensions,
  signing/provisioning workflows, RPC compatibility layers, legacy CLI aliases,
  forwarding exports for former module paths, or old-schema migrations.

## Workspace and scoped guidance

Dependencies flow from `rooklet` to both libraries, and from `rooklet-macos` to
`rooklet-core`. Import shared types directly from their owning crate. Shared
versions, edition, MSRV, license, and Rust lints live in the root Cargo.toml.

Read the applicable directory guidance before changing its files:

- [Core](crates/rooklet-core/AGENTS.md): platform-free models, schemas, and analysis.
- [macOS](crates/rooklet-macos/AGENTS.md): firewall operations and native observations.
- [Application](src/AGENTS.md): CLI, authentication, TUI state, workers, and rendering.

Also consult the application guide when changing CLI/TUI regressions under the
root `tests/` directory. Tests outside a source subtree still need its behavior rules.

Keep parsers, subprocess execution, native collection, and transaction internals
private. Split modules by cohesion; avoid catch-all utilities, speculative
abstractions, and a crate for every backend.

## Working approach

- For planning, review, or diagnosis, inspect and report; implement only when requested.
- Complete authorized work, make reasonable routine decisions, and preserve unrelated changes.
- Inspect relevant code before editing. Use targeted `rg` searches and batch independent reads.
- Keep changes focused and idiomatic. Prefer explicit types and small functions.
- Delegate substantial independent work with distinct ownership or a read-only scope.
  Agents share the workspace; coordinate edits and validate the integrated result.
- Keep README, CLI help, and scoped guidance synchronized with final behavior.
- Communicate meaningful progress and finish with actual validation and limitations;
  do not claim checks that were not run.

## Shared safety rules

- Use trusted executable paths and argument arrays; never interpolate user input into shell commands.
- Bound inputs, output, queues, history, and caches. Release resources on errors and shutdown.
- Installation/removal scripts change only the executable. Preserve quarantine;
  never bypass Gatekeeper or change live firewall settings, rules, or application data.
- Treat paths, process metadata, command output, and database content as untrusted.
  Sanitize human-readable output with core helpers; keep state and mutation targets unmodified.
- Keep unavailable, inactive, stale, partial, and observed states explicit.
  Do not fabricate readings, imply enforcement from observations, or bypass confirmations.
- Tests must not change the host firewall. Read-only queries and `pfctl -n` are appropriate.
  Live privileged mutation or connectivity tests require explicit user authorization,
  an isolated environment, and a concrete restoration procedure.
- Signal tests must target only processes they create. Implementation requests do not
  authorize live firewall activation or signaling unrelated processes.

## Validation

For code changes, run the macOS workspace checks from the repository root:

```sh
make check
make test
make package
```

These match macOS CI: workspace formatting, all-target compiler/Clippy checks,
shell syntax, regression tests, and an architecture-verified release archive.
Fix warnings at their cause; never add blanket suppression or weaken checks.
Use the core guide's portable checks when working on Linux, and report any macOS
validation that remains outstanding.

Add tests for meaningful behavior and failures, not implementation duplication.
For UI changes, render and inspect actual Ratatui cells at multiple terminal sizes.
For documentation-only changes, verify references and commands, and run
`git diff --check`; a full Rust rebuild is unnecessary.
