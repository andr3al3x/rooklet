# Core crate guidance

This guide applies to `crates/rooklet-core/`, including its tests. It supplements
the [root guidance](../../AGENTS.md) with requirements for the pure domain layer.

## Responsibilities

Module paths below are relative to this crate's `src/` directory.

- `model.rs`: shared observations, configuration schemas, and typed mutations.
- `process.rs` and `resources.rs`: captured identities, termination requests,
  resource readings, and pure grouping/projection logic.
- `permissions.rs`: captured path evidence and pure incoming-registration matching.
- `application.rs` and `profile.rs`: path syntax, bounded parsing, validation, and export.
- `network.rs` and `network/`: pure PF compilation and conservative rule analysis.
- `text.rs`: terminal-control and bidi sanitization.

## Constraints

- Production code must not query filesystems, launch subprocesses, access the
  network, call native APIs, or depend on either other workspace package.
  Keep Clap, Ratatui, Crossterm, and libc out of this crate.
- Data describes captured evidence; constructing an identity or permission map
  does not authorize a mutation. Live revalidation belongs to the macOS crate.
- Keep configuration schemas strict: reject unknown fields, unsupported formats,
  invalid scopes, and oversized input before callers persist or apply it.
- Keep enum spellings in one place. CLI parsing adapts core types in the application;
  do not derive CLI framework traits on domain types.
- Preserve ordered, first-match PF semantics. Destination means the remote peer;
  the service port is local inbound and remote outbound.
- Unknown query fields must remain indeterminate when an earlier rule could match.
  Shadow warnings describe complete coverage by one earlier rule, not combined coverage.
- Keep captured ownership and PID-generation distinctions intact when grouping.
  Missing, warming, partial, and stale resource data must not become fabricated values.

## Validation

Pure tests must run on macOS and Linux without system tools or privileges.
Cover malformed schemas, path syntax, rule scope/order, unknown fields, grouping,
and stale identity/evidence behavior with deterministic fixtures.

From the repository root, portable checks are:

```sh
cargo check -p rooklet-core --locked --all-targets
cargo clippy -p rooklet-core --locked --all-targets -- -D warnings
cargo test -p rooklet-core --locked
```

The root workspace checks verify consumers after public core API changes.
