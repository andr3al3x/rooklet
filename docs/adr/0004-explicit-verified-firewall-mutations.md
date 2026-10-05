# 0004: Make firewall mutations explicit, scoped, and verified

- Status: Accepted
- Recorded: 2026-10-05
- Scope: Authentication, ALF/PF authority, profiles, and restoration
- Supersedes: None
- Superseded by: None

## Context and constraints

Rooklet shares system firewall state with macOS and other software. Reviews can
become stale, system tools can fail after partial mutation, and ALF/PF have no
shared transaction. Abandoning an accepted PF helper can interrupt restoration.
This record captures the implemented preservation and verification contract.

## Decision

Use explicit user actions, typed requests, whole-proposal validation, and live
readback. Authenticate in the frontend; backend sudo is noninteractive. PF
requests use the canonical self-executable as a privileged helper when needed.
The helper's accepted transaction continues despite outer cancellation or capture
failure; bounded tool calls and final supervision still report failures.

Own only the managed PF anchor, metadata, and recorded enable reference. Setup
and removal support validated parent layouts, back up parent configuration, and
reject drift/unsafe files. Apply changes the managed anchor without global flush,
state removal, global disable, or routine parent reload.

Profile review captures an opaque baseline. Apply resolves/rechecks registrations,
revalidates that baseline, preflights PF before incoming changes when configured,
applies scopes, and verifies final state. Failures attempt restoration and report
both action and restoration outcomes. Do not promise ALF or multi-backend atomicity.

## Alternatives

- A global PF flush/disable or rewriting arbitrary parent configuration would
  simplify replacement at the cost of unrelated policy and service ownership.
- Killing the transaction helper on outer cancellation would shorten exit latency but
  could stop midway through persistence, application, or restoration.
- Applying only cached/reviewed state without revalidation would accept drift.
- Claiming atomic profile application would obscure sequential ALF failures.

## Consequences

Unrelated policy is preserved and stale/unsupported proposals are refused.
Custom layouts need explicit operator recovery rather than automatic rewriting.
Transactions and best-effort restoration add complexity, can delay shutdown, and
can still leave partial state that must be reported. Readback establishes
configuration consistency, not actual packet enforcement or connectivity.

## Verification and references

- [PF facade](../../crates/rooklet-macos/src/network.rs), [lifecycle](../../crates/rooklet-macos/src/network/lifecycle.rs), [persistence](../../crates/rooklet-macos/src/network/persistence.rs)
- [Helper supervision](../../crates/rooklet-macos/src/command/transaction.rs)
- [Incoming operations](../../crates/rooklet-macos/src/backend/alf/operations.rs)
- [Profile preparation](../../crates/rooklet-macos/src/profile/preparation.rs), [transactions and fake failure tests](../../crates/rooklet-macos/src/profile/transaction.rs)
- [PF operating guide](../network.md)

Use fake adapters for drift, partial failure, restoration, and cancellation;
`pfctl -n` is suitable for read-only syntax checks. Actual enforcement, coexistence,
and reboot behavior require explicitly authorized isolated integration testing.
