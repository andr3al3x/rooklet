# Documentation

| Document | Purpose |
| --- | --- |
| [Architecture](architecture.md) | Current package boundaries, runtime flows, trust, and persistence |
| [Functionality](functional.md) | Supported workflows, behavior, privileges, and product limits |
| [Architectural decisions](adr/README.md) | Decision history, trade-offs, status, and the ADR template |
| [Binary installation](install.md) | Standalone installation guide also shipped in release archives |
| [Builds and releases](releases.md) | CI artifacts, version tags, publication, and failed-release retries |
| [PF network rules](network.md) | Rule syntax, setup, ownership, drift, and status semantics |
| [Logging decision and contract](adr/0007-private-bounded-diagnostics.md) | Rationale, event levels, privacy, storage bounds, and shutdown behavior |

Start with the [repository README](../README.md) for installation and everyday
use. Architecture and functionality describe the implemented system; ADRs explain
its significant design choices. Detailed guides expand specific workflows rather
than duplicating those overviews.

Documentation maintenance rules live in the root [AGENTS.md](../AGENTS.md).
Keep this index current when documents are added, moved, or removed. Generated
previews, test output, release archives, and temporary research belong under
ignored `target/`, not in this directory.
