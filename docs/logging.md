# Diagnostic logging

The executable owns the `tracing-subscriber` configuration and log writer.
`rooklet-macos` emits typed `tracing` events; `rooklet-core` stays independent of
logging. With no `--log-dir`, there is no subscriber, writer thread, or log file.
Human diagnostics, CLI JSON, and TUI notices keep their existing output paths.

## Event map

| Level | Boundaries and fields |
| --- | --- |
| ERROR | Session/backend startup failures; failed changes or readback; failed PF/profile restoration; backend worker panic. Static operation/stage and restoration booleans identify the failure without error text. |
| WARN | Availability transitions, subprocess/update deadlines, partial termination, and cleanup failures. Fields include subsystem, timeout, delivery counts, and numeric OS error codes. |
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

## Privacy and output rules

Instrument fields explicitly. Never record raw `Debug`/`Display` of an operation,
configuration, snapshot, process, or error. Paths, app names, endpoints, rules,
profiles, arguments, passwords, and captured stdout/stderr are excluded at every
level. Executables are classified by a fixed allowlist of trusted tool paths;
other tools have the category `other`.

Only the `rooklet` and `rooklet_macos` target namespaces are enabled. `RUST_LOG`
does not change this policy or enable dependency logs. Privileged helpers are
invoked without logging flags and do not inherit a sink through environment
configuration. Run the whole application with logging options only when that
session itself needs diagnostic files.
For PF work delegated to a privileged helper, the parent records supervision and
the returned outcome; internal helper phases are absent from that log. Human
diagnostics still report application and restoration failures.

No logging path writes to stdout/stderr, including sink overflow or failure.
User-facing errors still contain their normal sanitized details. Diagnostic files
are intended for troubleshooting, not enforcement evidence or a durable audit.

## Storage and shutdown

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
  is sink bookkeeping and is emitted regardless of the selected event level.
- The application waits at most one second for sink completion. A stalled disk,
  failed sink, forced process exit, or crash can lose the tail and summary. No
  filesystem durability guarantee is made.

The small file writer is intentional: the examined [`tracing-appender` 0.2.5 shutdown
implementation](https://docs.rs/tracing-appender/0.2.5/src/tracing_appender/non_blocking.rs.html)
can print timeout diagnostics to stdout. Keeping CLI JSON and the
terminal clean also requires whole-record, file-size, and private-directory
bounds beyond the subscriber's formatter.

## Validation

Tests cover disabled logging, command exit codes/output, private configuration
exclusion, severity/namespace filtering, span context, bounded storage, retention,
symlink/permission rejection, overflow, write failure, and shutdown flushing.
Backend capture tests use harmless subprocesses and fake profile transactions;
logging verification must never mutate the machine's live firewall.
