# Builds and GitHub releases

Rooklet publishes macOS binary packages as assets on
[GitHub Releases](https://github.com/andr3al3x/rooklet/releases). Each version has
separate Apple silicon (`arm64`) and Intel (`x86_64`) archives, each with its own
SHA-256 file. Follow the [binary installation guide](install.md) to install them.

## Checks and build artifacts

The [Checks workflow](../.github/workflows/check.yml) runs for pull requests and
pushes to `main`, and can be started manually from the Actions tab. It runs:

- `make check`, `make test`, and architecture-specific `make package` on native
  `macos-15` Apple silicon and `macos-15-intel` Intel runners.
- Portable core Clippy/tests and workflow linting on Ubuntu.
- Full workspace compiler checks with the declared Rust 1.88 minimum version.

Successful macOS jobs upload an archive/checksum pair under
`rooklet-macos-arm64` or `rooklet-macos-x86_64`. Actions retains these build
artifacts for 14 days; they are not GitHub releases. Published release assets
remain available independently of Actions artifact retention.

## Publish a version

1. Update `workspace.package.version` in the root `Cargo.toml`, update
   `Cargo.lock` with `cargo check --workspace --all-targets`, and commit the
   version change with the release contents. Internal library versions share
   that workspace version.
2. Run `make check`, `make test`, and `make package`, then push the commit to
   `main` and wait for Checks to pass.
3. Create and push a tag that exactly matches `v<Cargo version>`. For the current
   version:

   ```sh
   git tag -a v0.2.0 -m "Rooklet 0.2.0"
   git push origin v0.2.0
   ```

The [Release workflow](../.github/workflows/release.yml) checks the tag/version
match, calls the same Checks workflow on the tagged commit, and waits for every
job to pass. It verifies exactly these four assets before uploading:

```text
rooklet-0.2.0-macos-arm64.tar.gz
rooklet-0.2.0-macos-arm64.tar.gz.sha256
rooklet-0.2.0-macos-x86_64.tar.gz
rooklet-0.2.0-macos-x86_64.tar.gz.sha256
```

Publication uses the workflow's `GITHUB_TOKEN`; no personal access token or
additional secret is required. Only the publish job requests `contents: write`.
The workflow creates a draft with generated release notes, uploads the assets,
and publishes after all uploads succeed. A Cargo prerelease version such as
`0.3.0-rc.1` creates a GitHub prerelease; build metadata alone does not.

## Retry a failed release

Correct a transient failure and use **Re-run all jobs** on the original Release
run. A matching draft is reused and its expected assets are replaced by the
verified build. A published release is left unchanged. An invalid version/tag
match must be corrected before creating the intended release tag.

Builds and checks do not mutate the host firewall. Release automation retains
the existing binary-only scope and Gatekeeper/quarantine behavior. The
[release decision](adr/0010-verified-native-release-publication.md) describes
the validation and publication boundary.
