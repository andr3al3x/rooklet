# 0005: Observe locally and collect resources on demand

- Status: Accepted
- Recorded: 2026-10-05
- Scope: Traffic accounting, resource sampling, and freshness
- Supersedes: None
- Superseded by: None

## Context and constraints

Activity needs useful per-app traffic and process details with low recurring cost.
Process summaries, flow counters, helper exits, PID reuse, and inaccessible data
can produce misleading totals or rates. This record describes the existing
local observation pipeline and resource-interest model.

## Decision

Keep one persistent `nettop` monitor with bounded incremental parsing. Treat
process summaries as authoritative, retain helper contributions, separate rates
from totals, and group apps using verified paths. Preserve unavailable/local/
unknown and partial/stale states instead of inventing values or firewall verdicts.

Collect native resource counters only for display, inspection, or resource sort
interest. Discovery and sampling are incremental, bounded, and prioritize visible
apps while rotating other work fairly. Current limits include a 4096-process cap,
two-second counter interval, five-second discovery interval, and cooperative 20 ms
work budget; a single native call can exceed that budget. Disabling interest or
changing identity resets rate baselines and does not stop `nettop`.

Cache routine firewall controls briefly with explicit age; explicit snapshots,
reviews, refreshes, and mutation readback force fresh reads.

## Alternatives

- Polling new traffic commands per frame would increase process overhead and make
  rate baselines and helper accounting harder to preserve.
- Sampling every process continuously would spend work when nobody consumes it.
- Parsing a separate process-monitoring tool would add subprocess/format overhead
  for counters already available through narrow native APIs.
- Treating missing data as zero would look simple but falsely imply inactivity.

## Consequences

Idle/hidden resource work is reduced and frame rendering stays pure. Observation
is sampled and can lag during explicit work; discovery and coverage can be partial.
CPU is per-core, memory is an estimate, and rates need valid consecutive samples.
The native adapter requires reviewed unsafe contracts and explicit budget limits;
a cooperative time budget cannot guarantee a hard upper bound for OS calls.

## Verification and references

- [Activity monitor](../../crates/rooklet-macos/src/activity/monitor.rs), [accounting](../../crates/rooklet-macos/src/activity/tracker.rs)
- [Control cache](../../crates/rooklet-macos/src/backend/cache.rs)
- [Resource engine](../../crates/rooklet-macos/src/resources/engine.rs), [native adapter](../../crates/rooklet-macos/src/resources/native.rs), [resource tests](../../crates/rooklet-macos/src/resources/tests.rs)
- [Viewport interest](../../src/ui.rs), [functional semantics](../functional.md#traffic-countries-and-resources)

Test resets, helper exits, grouping, sample freshness, fairness, and interest
changes with captured fixtures/fake counters. The existing ignored native timing
diagnostic is read-only; its results are host-specific, not a universal cost bound.
