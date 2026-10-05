# 0009: Supervise privileged tools inside their owning root helper

- Status: Accepted
- Recorded: 2026-10-05
- Scope: Incoming mutations, PF helper reads, subprocess ownership, and shutdown
- Supersedes: None
- Superseded by: None

## Context and constraints

The normal-user process cannot reliably kill root descendants of sudo. Sudo may
fork its command or use a separate terminal session, and cannot relay SIGKILL.
An outer timeout can therefore return while a privileged tool continues working.
Readback and restoration must run after managed tool cleanup, and PF restoration
must continue after acceptance. Delivery remains one Rust executable.

This decision extends [ADR 0004](0004-explicit-verified-firewall-mutations.md)'s
existing PF helper boundary to incoming operations and privileged reads.

## Decision

Send incoming changes to the canonical current executable through noninteractive
sudo. A hidden root-only CLI entry point accepts a strict private request of at
most 1 MiB: one setting change, application permission changes for at most 256
unique paths, addition, or removal. Requests cannot specify an executable, command
arguments, or PF operations. Validate syntax on both sides, then perform existing
live target validation and readback inside the helper.

Dispatch this entry point before normal backend observations or diagnostic sink
initialization, and reject logging options. The macOS crate owns request decoding
and ALF operations. The application only forwards bounded input to its facade.
Diagnostic privacy remains governed by [ADR 0007](0007-private-bounded-diagnostics.md).

The root helper directly owns bounded tool subprocesses and cleans their isolated
groups before reaping the leader. The ordinary bounded runner refuses privileged
sudo wrapping from a normal-user process. Incoming and PF helpers are supervised
without an outer kill deadline; cancellation before launch prevents execution,
and acceptance commits the parent to waiting for the helper. Capture overflow and
I/O failures are reported after completion. Read-only PF status/preflight requests
use the same supervision so their bounded root tools are not orphaned by an
outer timeout.

## Alternatives

- Relaying TERM through sudo and later sending KILL still depends on sudo policy,
  signal permissions, and session layout for descendant cleanup.
- A resident privileged service adds installation and persistent authority outside
  the standalone delivery boundary.
- Running the whole terminal application as root gives observation and interaction
  code unnecessary privileges.

## Consequences

Tool deadlines and cleanup are enforced by a process with the tool's privileges.
An incoming operation may partially change ALF state, and successful cleanup does
not prove readback or restoration succeeded. Parent errors retain those failures.
Helpers may extend shutdown latency; individual subprocess bounds are not a hard
deadline on filesystem or kernel stalls. PF remains an unsupported product API
with the [documented compatibility limits](../network.md#scope-and-compatibility).

## Verification and references

- [Incoming facade and private requests](../../crates/rooklet-macos/src/backend/incoming.rs)
- [Dedicated CLI dispatch](../../src/cli/mod.rs), [CLI regressions](../../tests/cli.rs)
- [Tool runner](../../crates/rooklet-macos/src/command.rs), [supervision tests](../../crates/rooklet-macos/src/command/tests.rs)
- [PF facade](../../crates/rooklet-macos/src/network.rs)
- [macOS sudo manual](https://www.sudo.ws/docs/man/sudo.man/)

Tests exercise strict request validation, root rejection, absence of helper logs,
prelaunch cancellation, owned-child timeout cleanup, and completion despite
cancellation/input/capture failures using harmless fixtures. Actual sudo/session
behavior needs explicitly authorized isolated testing with harmless privileged
children; these fixtures do not establish live firewall enforcement.
