# 0002: Separate domain, macOS operations, and application responsibilities

- Status: Accepted
- Recorded: 2026-10-05
- Scope: Cargo workspace and public surfaces
- Supersedes: None
- Superseded by: None

## Context and constraints

Validation and analysis must be testable independently of host state. Native
collection, persistence, mutation, and interaction have different responsibilities
and dependencies. Splitting every small backend into a public crate would expand
coordination and API surface. This record captures the existing separation.

## Decision

Maintain three packages: pure `rooklet-core`, platform-owned `rooklet-macos`, and
the `rooklet` application. Dependencies flow application → both libraries and
macOS → core, never in reverse. Core owns schemas, captured evidence, pure
validation/analysis/projection, and text sanitization. macOS owns live evidence,
system operations, and persistence. The application owns CLI adaptation,
authentication, state/effects, rendering, coordination, and diagnostic setup.

Expose typed operations through small facades; keep parsers, command execution,
native wrappers, and transaction internals private. Import shared types directly
from their owning crate. Shared versions and lints live in root Cargo metadata.
Internal libraries are not separately published APIs or compatibility surfaces.

## Alternatives

- One package with broadly shared modules makes it easier for pure logic to acquire
  system/UI dependencies and widens review context.
- A crate for each backend gives finer boundaries but introduces public surfaces
  without a distinct consumer or ownership need.
- Forwarding old module paths would reduce migration effort but preserve obsolete
  architecture contrary to the clean-break policy.

## Consequences

Core tests run on Linux without macOS tools, and ownership is enforced through
dependency direction. Frontend/platform integration must use explicit shared
types and typed facades. Internal module changes can remain local, while shared
API changes require consumer checks. Crate boundaries do not justify speculative
abstractions or duplicating types and validation.

## Verification and references

- [Workspace manifest](../../Cargo.toml), [core manifest](../../crates/rooklet-core/Cargo.toml)
- [Core facade](../../crates/rooklet-core/src/lib.rs), [macOS facade](../../crates/rooklet-macos/src/lib.rs)
- [Scoped guidance](../../AGENTS.md#workspace-and-scoped-guidance)
- [CI checks](../../.github/workflows/check.yml)

Use portable core tests plus full workspace consumer checks when shared types or
dependencies change. Review source ownership as well as the Cargo graph: keeping
an API private does not alone prevent a layer from doing inappropriate work.
