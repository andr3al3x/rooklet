# 0008: Bind confirmed process signals to captured kernel identity

- Status: Accepted
- Recorded: 2026-10-05
- Scope: Process termination authority and stale targets
- Supersedes: None
- Superseded by: None

## Context and constraints

Processes can exit, change executable, or have their PID reused between display,
confirmation, and signaling. App grouping alone cannot authorize terminating a
newly discovered helper. Termination can lose user work and must protect unrelated
or privileged processes. This record captures the implemented safety boundary.

## Decision

Confirm the exact captured target list and `SIGTERM` or separately reviewed
`SIGKILL`. Include captured helpers supported by verified identities, never
rediscover targets during delivery. Reject root-owned/non-current-user targets,
Rooklet itself, protected ancestors, duplicates, invalid identities, and stale
requests. Recheck executable/bundle path, ownership, start time, and kernel PID
generation, then bind delivery to that generation. Never fall back to PID-only
signaling when identity-bound support is unavailable. Report partial delivery and
do not equate signal delivery with process exit.

## Alternatives

- A PID-only kill leaves a reuse race even after a userspace identity check.
- Killing by display name or dynamically expanding the app group can reach targets
  that were never captured and confirmed.
- Escalating privileges to terminate other users' or root-owned processes exceeds
  the product's current-user termination scope.

## Consequences

Confirmation stays bound to actionable evidence rather than labels or stale PIDs.
Some requests or hosts are refused, and running the app as root disables
termination. Per-target failures can yield partial delivery. Apps may ignore
`SIGTERM`, restart after delivery, or lose unsaved work; successful delivery does
not establish completed termination.

## Verification and references

- [Captured identities and requests](../../crates/rooklet-core/src/process.rs)
- [Revalidation engine](../../crates/rooklet-macos/src/process/engine.rs), [native signaling](../../crates/rooklet-macos/src/process/native.rs)
- [Frontend confirmation](../../src/app/termination.rs), [process tests](../../crates/rooklet-macos/src/process/tests.rs)
- [macOS safety guidance](../../crates/rooklet-macos/AGENTS.md#firewall-and-process-safety)

Use fake adapters for stale identities and partial failures. Native signal tests
must target only their own spawned children; do not test this decision by
signaling unrelated live applications.
