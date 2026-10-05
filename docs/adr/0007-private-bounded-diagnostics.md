# 0007: Use optional private, bounded file diagnostics

- Status: Accepted
- Recorded: 2026-10-05
- Scope: Diagnostic dependencies, output, privacy, and shutdown
- Supersedes: None
- Superseded by: None

## Context and constraints

CLI, worker, subprocess, and restoration failures need correlated diagnostics.
Console logging can corrupt terminal rendering or CLI JSON. Raw arguments,
observations, and error display can expose private data. Diagnostic disk work must
not block firewall operations. This record documents the implemented logging
choice and its dependency trade-off.

## Decision

Use `tracing` events/spans with `tracing-subscriber` owned by the executable.
`rooklet-macos` emits explicit structured fields; `rooklet-core` stays independent
of logging. With no `--log-dir`, there is no subscriber, writer thread, or log file.
Human diagnostics, CLI JSON, and TUI notices retain their existing output paths.

Use a small private file writer with nonblocking bounded enqueueing, whole-record
limits, run-file caps, retention, private/no-follow storage, loss counters, and
silent I/O failure. Drain after terminal restoration and backend completion with
a one-second wait limit. Privileged helpers do not inherit the sink.

### Event map

| Level | Boundaries and fields |
| --- | --- |
| ERROR | Session/backend startup failures; failed changes or readback; failed PF/profile restoration; backend worker panic. Static operation/stage and restoration booleans identify the failure without error text. |
| WARN | Availability transitions, subprocess/update deadlines, partial termination, cleanup failures, and dropped records. Fields include subsystem, timeout, delivery/loss counts, and numeric OS error codes. |
| INFO | Session and explicit operation lifecycle, authentication outcome, successful verification/restoration, explicit GeoIP update, and availability recovery. Fields include operation category/ID, outcome, counts, and duration. |
| DEBUG | Validation/transaction phases, fixed tool category, subprocess completion/exit status, normal cancellation, monitor/worker lifecycle, and shutdown waiting. |
| TRACE | Aggregate observation duration and sample/application/resource counts. No individual connection or process records. |

Availability events describe transitions rather than repeating a warning on each
poll. Independent subsystems can be unavailable while the overall observation
still succeeds. Successfully observed traffic does not imply a firewall verdict.

Session and operation spans correlate events, including work performed by the TUI
backend thread. A CLI operation uses ID 1; queued TUI operations use successive
IDs within that session. IDs are diagnostic context, not persisted identifiers.
Verbosity filters may omit lower-level spans; failure events retain their static
operation or phase fields where relevant.

### Privacy and output rules

Instrument fields explicitly using static categories, counts, durations, numeric
status, and outcomes. Never record raw `Debug`/`Display` of an operation,
configuration, snapshot, process, or error. Paths, app names, endpoints, rules,
profiles, arguments, passwords, and captured stdout/stderr are excluded at every
level. Executables are classified by a fixed allowlist of trusted tool paths;
other tools have the category `other`.

Only the `rooklet` and `rooklet_macos` target namespaces are enabled. `RUST_LOG`
does not change this policy or enable dependency logs. Privileged helpers are
invoked without logging flags and do not inherit a sink through environment
configuration. For delegated PF work, the parent records supervision and the
returned outcome; internal helper phases are absent from that log. Human
diagnostics still report application and restoration failures.

No logging path writes to stdout/stderr, including sink overflow or failure.
User-facing errors retain their normal sanitized details. Diagnostic files are
intended for troubleshooting, not enforcement evidence or a durable audit.

### Storage and shutdown contract

- Explicit directory, current-user ownership, private permissions (`0700` for
  newly created directories, `0600` for files), and no-follow file operations.
- A separate JSONL file per run; startup prunes matching older run files to retain
  five including the new file. Unrelated files are left alone. Very old active
  runs can be pruned; use separate directories when independent retention matters.
- At most 512 queued records, 16 KiB per record, and 4 MiB per run file, including
  reserved space for the final counters. Oversized records are dropped whole.
- Producers enqueue without waiting for disk I/O. Overflow drops events and counts
  them rather than delaying firewall transactions or rendering.
- Shutdown starts after backend workers finish and terminal state is restored.
  The sink drains accepted records and writes a final `logging_finished` record
  containing queue/record/file-budget drops and write-error counts. This summary
  is sink bookkeeping and is emitted regardless of the selected event level;
  its level is WARN on loss and INFO otherwise.
- The application waits at most one second for sink completion. A stalled disk,
  failed sink, forced process exit, or crash can lose the tail and summary. No
  filesystem durability guarantee is made.

## Alternatives

- Console `log`/`env_logger` output would require additional routing to avoid
  corrupting the TUI or machine-readable output and lacks the chosen span context.
- `tracing-appender` supplies a bounded background queue, but its examined 0.2.5
  guard shutdown can print timeout diagnostics to stdout. It also does not provide
  the complete whole-record/file-size/private-directory contract required here.
- A synchronous file writer would make disk stalls part of mutation/render latency.
- Logging raw payloads would expose private observations and configuration.

## Consequences

Operation context and failures are observable without changing terminal output.
Privacy intentionally limits detail; human errors remain the place for sanitized
payload diagnostics. The custom sink is extra maintained code, constrained to
diagnostic storage rather than a general logging framework. Overflow, crashes,
failed/stalled filesystems, and shutdown timeout can lose records or the final
summary. These logs are best effort, not a durable audit trail. Inner phases of
privileged PF helpers are absent from the parent's log.

## Verification and references

- [Subscriber setup](../../src/logging.rs), [bounded sink](../../src/logging/sink.rs), [private directory operations](../../src/logging/directory.rs)
- [Runtime tests](../../src/logging/tests.rs), [CLI regressions](../../tests/logging.rs), [backend capture tests](../../crates/rooklet-macos/src/command/tests.rs)
- [User-facing logging options](../../README.md#diagnostic-logging)
- [Examined appender shutdown source](https://docs.rs/tracing-appender/0.2.5/src/tracing_appender/non_blocking.rs.html)

Tests cover output/exit preservation, filtering, span context, payload exclusion,
private storage, limits, loss, write failure, and flushing. Reconsidering the writer
dependency requires preserving those behaviors, including failure-time silence.
Backend capture tests use harmless subprocesses and fake profile transactions;
logging verification must never mutate the machine's live firewall.
