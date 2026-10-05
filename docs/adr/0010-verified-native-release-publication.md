# 0010: Verified native builds and tag-driven release publication

- Status: Accepted
- Recorded: 2026-10-05
- Scope: Continuous integration and binary distribution
- Supersedes: None
- Superseded by: None

## Context and constraints

The existing CI checks one macOS runner and builds an archive locally, but has
no release publication flow. Binary delivery requires separate Apple silicon
and Intel packages; compilation alone does not exercise native process and
installer behavior on the other architecture. Release assets must correspond
to the tested source and Cargo version. Internal crates remain workspace
implementation packages under [ADR 0001](0001-standalone-terminal-delivery.md).

## Decision

Reuse one Checks workflow for pull requests, `main` pushes, manual builds, and
release tags. Run full workspace validation and packaging on explicit native
macOS 15 ARM and Intel runners. Retain Linux core validation and check the full
workspace against Rust 1.88. Lint the workflow definitions. Upload each native
archive/checksum pair as a uniquely named, bounded-retention Actions artifact.

Release publication starts only from a pushed `v<Cargo version>` tag. Reject a
version mismatch before builds, then gate publication on all Checks jobs for
the tagged source. Download only the current run's artifacts, require exactly
both archive/checksum pairs, and verify their SHA-256 contents. Publish through
GitHub CLI: create or resume a matching draft, upload the verified assets, then
publish. Preserve already-published releases on reruns. Mark SemVer prerelease
versions as prereleases, excluding build metadata from that classification.

Keep build and validation jobs read-only. Grant `contents: write` only to the
publish job, which does not check out or compile repository code. Pin action
dependencies to full upstream commit IDs and disable persisted checkout
credentials. Release jobs use the run's `GITHUB_TOKEN`, without custom secrets.
Use distinct concurrency groups for reusable checks and publication; an active
release is not cancelled by a newer run for the same tag.

## Alternatives

- Cross-compiling both packages on ARM saves a native runner but leaves Intel
  tests and installation behavior unchecked.
- Separate release build commands can drift from PR validation. A reusable
  workflow gives both entry points the same checks and package construction.
- Publishing assets before checks finish exposes an incomplete release. Draft
  uploads keep incomplete publication recoverable and support immutable releases.
- Publishing internal crates or signed app bundles changes the established
  product boundary without serving binary distribution.

## Consequences

Each release reruns validation on its tagged source. Native Intel CI consumes
an additional runner, and hosted runner labels/action pins need maintenance.
Actions artifacts expire; release assets are independent of that retention.
Failed uploads can leave a recoverable draft. Published assets are not replaced
by reruns. Quarantine, lack of Developer ID/notarization, and explicit firewall
privileges remain unchanged.

## Verification and references

- [Checks workflow](../../.github/workflows/check.yml)
- [Release workflow](../../.github/workflows/release.yml)
- [Packaging script](../../scripts/package.sh), [release instructions](../releases.md)
- [Hosted runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
- [Action pinning guidance](https://docs.github.com/en/actions/reference/security/secure-use)
- [GitHub release creation](https://cli.github.com/manual/gh_release_create)

Validate workflow syntax, tag rejection/prerelease classification, missing or
tampered assets, and draft/publication failure behavior without remote writes.
Local macOS workspace/package checks and the Rust 1.88 compiler check are
available; actual hosted execution and release publication still require a
GitHub run. Native CI is not live privileged firewall integration testing.
