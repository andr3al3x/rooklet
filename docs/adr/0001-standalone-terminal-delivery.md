# 0001: Standalone Rust terminal delivery with explicit backend scopes

- Status: Accepted
- Recorded: 2026-10-05
- Scope: Product authority and distribution
- Supersedes: None
- Superseded by: None

## Context and constraints

Rooklet needs a compact terminal interface for macOS firewall management and
local activity inspection. Delivery must remain one Rust executable without an
extension, app bundle, resident privileged service, or signing/notarization
workflow. The existing system tools expose different kinds of control and
observation. This record describes the implemented design retrospectively.

## Decision

Use Clap for explicit commands and Ratatui/Crossterm for keyboard and mouse
interaction. Distribute architecture-specific binary archives with installation
scripts, checksum, license, and a short guide.

Keep backend capabilities explicit: `socketfilterfw` controls incoming ALF
registrations/settings; PF supplies machine-wide network rules; `nettop` supplies
sampled observations. Run as the normal user and authenticate specific privileged
operations through normal sudo. Do not claim outgoing per-app enforcement,
per-connection approval, or complete blocked-attempt telemetry.

## Alternatives

- A GUI/system extension could support different enforcement and event capture,
  but introduces platform packaging and provisioning outside this product scope.
- A resident privileged daemon would introduce installation, lifecycle, and IPC
  authority that the single-executable workflow does not need.
- Treating every backend as one firewall would simplify labels but misrepresent
  incoming permissions, PF policy, and observed traffic.

## Consequences

Distribution and removal stay small, and system authority remains visible.
The app inherits the limitations of native tools and sampled observations.
Authentication is explicit; downloaded binaries may encounter Gatekeeper, and
quarantine must remain intact. macOS's linker can add an ad-hoc signature; absence
of a Developer ID/notarization workflow does not imply absence of all signatures.
Removing the executable does not remove live firewall configuration or data.

## Verification and references

- [CLI arguments](../../src/cli/args.rs), [terminal lifecycle](../../src/tui/terminal.rs)
- [Backend facades](../../crates/rooklet-macos/src/lib.rs)
- [Release scripts](../../scripts/), [binary guide](../install.md)
- [Functional boundaries](../functional.md#distribution-and-current-limits)

Verify archives and installation behavior through the existing packaging checks.
Privilege/enforcement and Intel runtime behavior still need isolated integration
coverage; packaging success does not establish those capabilities.
