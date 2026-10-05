# Architectural decision records

ADRs record significant architectural choices, their constraints, alternatives,
and consequences. The [architecture overview](../architecture.md) describes the
current design; [functionality](../functional.md) describes supported behavior.

## Records

| ID | Decision | Status |
| --- | --- | --- |
| [0001](0001-standalone-terminal-delivery.md) | Standalone Rust terminal delivery with explicit backend scopes | Accepted |
| [0002](0002-workspace-responsibilities.md) | Separate domain, macOS operations, and application responsibilities | Accepted |
| [0003](0003-bounded-system-work.md) | Keep system work in a bounded TUI worker | Accepted |
| [0004](0004-explicit-verified-firewall-mutations.md) | Make firewall mutations explicit, scoped, and verified | Accepted |
| [0005](0005-local-demand-driven-observations.md) | Observe locally and collect resources on demand | Accepted |
| [0006](0006-offline-country-data.md) | Resolve countries offline with explicit database updates | Accepted |
| [0007](0007-private-bounded-diagnostics.md) | Use optional private, bounded file diagnostics | Accepted |
| [0008](0008-identity-bound-process-signaling.md) | Bind confirmed process signals to captured kernel identity | Accepted |
| [0009](0009-privilege-owned-tool-supervision.md) | Supervise privileged tools inside their owning root helper | Accepted |

Records 0001–0008 are retrospective descriptions of implemented decisions at
the time of recording. Their recorded date is not an asserted historical decision
date. Alternatives explain the trade-offs of the current design; they do not
claim to reconstruct a past debate or approval by named people.

## Creating and maintaining records

1. Copy [TEMPLATE.md](TEMPLATE.md) to the next unused four-digit number and a short
   kebab-case title: `NNNN-short-decision.md`. Never renumber or reuse an ID.
2. Keep one cohesive decision per record. Describe the concrete problem and
   constraints, the chosen approach, credible alternatives, and costs as well as
   benefits. Link implementation and relevant validation without claiming unrun
   checks. Do not create ADRs for routine local refactors.
3. Use `Proposed` for an unresolved choice and `Accepted` for an adopted design.
   An accepted record documents a decision, not proof that every planned detail
   has shipped; explicitly state implementation gaps and keep overviews factual.
4. Preserve accepted decision history. Corrections and clarifications can edit
   an existing record. A materially different choice needs a new ADR: mark the
   old one `Superseded`, link `Superseded by`, and link `Supersedes` in the new one.
   Mark rejected proposals `Rejected`; use `Deprecated` for a retired choice with
   no replacement. Keep all records and update this index.
5. Synchronize [architecture.md](../architecture.md),
   [functional.md](../functional.md), and affected guides with implemented changes,
   following the root [documentation rules](../../AGENTS.md#documentation-maintenance).

Keep decision rationale and defining contracts here. User-facing operating
instructions belong in README/guides; architectural summaries link ADRs rather
than repeating alternatives. ADR 0007 is the canonical logging contract and
event map; do not create a second logging specification outside it.
