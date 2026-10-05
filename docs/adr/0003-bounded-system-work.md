# 0003: Keep system work in a bounded TUI worker

- Status: Accepted
- Recorded: 2026-10-05
- Scope: Interaction, concurrency, and shutdown
- Supersedes: None
- Superseded by: None

## Context and constraints

System calls, subprocesses, profile storage, and explicit downloads can take
longer than a frame. Queued polling must not grow without bound or starve an
accepted mutation. Quitting must restore the terminal and finish authorized
transactions. This record describes the implemented worker model.

## Decision

Render cached `App` state and route input into typed effects. One backend worker
owns system observations and executes explicit work. Work and response channels
each have capacity one; only one explicit operation is in flight. Accepted work
has priority, ordinary observations can be coalesced, and resource interest and
refresh requests replace shared state rather than enqueueing repeated work.

Suspend raw mode and mouse capture for frontend sudo authentication. Publish
action and refreshed-observation outcomes separately, even after a partial action
failure. On exit release blocked response delivery, wait for accepted work, and
restore terminal state. Drain diagnostics after both cleanup paths finish.

## Alternatives

- Blocking the input/render loop on system work would make slow commands visible
  as UI stalls and complicate terminal authentication.
- An unbounded job queue would retain obsolete sampling work and delay mutations.
- Multiple independent mutation workers could overlap host-state transactions and
  reviewed baselines without providing a needed product capability.

## Consequences

Rendering remains independent of system latency and memory use stays bounded.
Explicit operations serialize, and samples can be delayed or coalesced during
backend work. Quitting may wait for a transaction; cancellation is not permission
to abandon restoration. A fresh snapshot can fail after a successful action, so
UI notices must preserve both facts rather than roll them into generic success.

## Verification and references

- [Event routing](../../src/tui.rs), [worker coordination and tests](../../src/tui/worker.rs)
- [Terminal lifecycle](../../src/tui/terminal.rs), [interaction state](../../src/app.rs)
- [Application guidance](../../src/AGENTS.md)
- [Transaction boundary](0004-explicit-verified-firewall-mutations.md)

Existing tests cover accepted work on shutdown, bounded observation delivery,
action/readback errors, confirmation, and cached-state interaction. UI changes
also need actual cell rendering and pointer-geometry inspection.
