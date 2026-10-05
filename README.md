# Rooklet

A compact Rust TUI for macOS firewall settings, network rules, and app traffic.
Manage incoming permissions, inspect connections and countries, and apply
machine-wide network rules from one terminal.

One executable with keyboard and mouse navigation.

## Quick start

Requires **macOS**, **Rust 1.88+**, and the macOS command-line build tools.
Clone this repository, open its directory, and install from source:

```sh
cargo install --path . --locked
```

Run as your normal user:

```sh
rooklet doctor
rooklet
```

Launching Rooklet does not activate or change either firewall. Administrator
access is requested when needed through normal `sudo` authentication; you do
not need to run the whole app with `sudo`.

Without installing, use `cargo run --release --locked --`. Use at least **80 × 24**
for comfortable navigation; the minimum is 50 × 17. No patched font is required.

## Binary installation (no Rust required)

Download packages and checksums from
[GitHub Releases](https://github.com/andr3al3x/rooklet/releases).
Each archive includes a [short installation guide](docs/install.md), the binary,
install/uninstall scripts, and the license.

Use the archive for your Mac: `macos-arm64` for Apple silicon, or
`macos-x86_64` for Intel. With the archive and its matching `.sha256` file in the
same directory, verify and extract it. For version 0.2.0 on Apple silicon:

```sh
shasum -a 256 -c rooklet-0.2.0-macos-arm64.tar.gz.sha256
tar -xzf rooklet-0.2.0-macos-arm64.tar.gz
cd rooklet-0.2.0-macos-arm64
sh install.sh
export PATH="$HOME/.local/bin:$PATH"
rooklet --version
```

The bundled installer needs only macOS's standard tools; neither Rust nor Xcode
command-line tools are needed. It copies the executable
to `~/.local/bin/rooklet`, sets executable permissions, and atomically replaces an
existing regular file for upgrades. It does not request sudo, run Rooklet, edit
shell configuration, or change firewall settings or country data. Add the PATH
line to your shell configuration if this directory is not already on PATH.

Choose another writable destination with `sh install.sh --bin-dir /absolute/path/to/bin`.
The installer rejects a binary for a different architecture and refuses to
replace a destination symlink or directory. Run it again with a newer archive
to upgrade. `sh install.sh --help` lists the options.

Release archives have no Developer ID signature or notarization. macOS's linker
may supply an ad-hoc signature. Downloaded binaries can be blocked by Gatekeeper;
see [Apple's guidance for opening downloaded software](https://support.apple.com/en-us/102445).
The installer preserves quarantine attributes. Source installation remains an
option without a downloaded binary.

To uninstall a binary installation, use the script from an extracted archive:

```sh
sh uninstall.sh
# For a custom installation directory:
sh uninstall.sh --bin-dir /absolute/path/to/bin
```

Uninstall removes only the `rooklet` executable. Applied firewall settings, PF
rules, backups, and country data remain. If you want to remove Rooklet's PF setup,
explicitly run `rooklet network remove` before uninstalling. For an installation
made with `cargo install`, use `cargo uninstall rooklet` instead.

## Features and scope

| View | What you can do | Backend and scope |
| --- | --- | --- |
| Activity | View app and helper traffic, peers, countries, resource usage, and process details; review process termination | Observed traffic from `nettop` and native macOS resource counters |
| Applications | Add, allow, block, and remove registered incoming permissions | macOS application firewall (`socketfilterfw`) |
| Network | Add, edit, reorder, and toggle IP/CIDR, port, protocol, direction, and interface rules | PF rules for every application on the Mac |
| Settings | Turn the incoming firewall on/off; manage stealth, block-all, signed-app defaults, and country data | macOS application firewall and local GeoIP database |

Rooklet also provides JSON status, configuration profiles, rule previews/explanations, and
light/monochrome themes:

```sh
rooklet status
rooklet --theme light
rooklet --theme mono
```

**Status: early release.** Local checks cover the Rust implementation, native
process identity/signaling, live observation, and terminal interaction on Apple
silicon. Privileged firewall mutation, actual blocked/allowed connectivity,
VPN coexistence, and reboot behavior still need isolated integration testing.
Intel Macs and older macOS versions have not been verified.

The built-in controls do not provide outgoing per-app blocking or interactive
approval of every new connection. Activity is sampled observation, not packet
capture, a firewall verdict, or a complete log of blocked attempts. An allow
rule in one firewall does not bypass another firewall or PF anchor. Automatic
startup and timed network-rule expiry are not implemented.

## Incoming firewall

In Settings, select **Application firewall**, press `Enter`, and review the
on/off change. Applications lets you register a bundle or executable with `n`
and review incoming permissions with `a` / `b`.

The following CLI commands read state or explicitly request changes:

```sh
rooklet firewall status
rooklet apps list
rooklet apps add /Applications/Example.app
rooklet apps block /Applications/Example.app
rooklet apps allow /Applications/Example.app
rooklet apps remove /Applications/Example.app
rooklet firewall set firewall on
rooklet firewall set stealth on
```

Replace the example with an installed app. A bundle is resolved through its
`CFBundleExecutable` metadata to the executable macOS registers. CLI
allow/block/remove preserve an exact listed path or resolve a bundle to its
registered main executable; they affect one registration.

Wide Activity rows show incoming permissions as `Allow`, `Block`, `Mixed`,
`Unlisted`, or `Unknown`. `Mixed` means registered entries in the same verified
app bundle have different permissions. Activity's `a` / `b` actions confirm
all registered entries for that bundle; Applications actions affect the selected
entry. These labels describe listed permissions, not an observed packet verdict.

TUI firewall changes require confirmation. Scroll long confirmations with ↑↓,
Page Up/Down, Home/End, or the mouse wheel to review every target. Rooklet validates
targets before changing them and verifies each result. Application firewall
operations are sequential; partial failures identify affected entries.
Failed operations trigger a status readback. If that readback is unavailable,
cached firewall controls become unavailable until a successful refresh.

Authentication happens outside terminal raw mode. Rooklet never collects or stores
passwords, and worker commands use noninteractive `sudo`. Quitting waits for an
authorized mutation to finish. Applied settings remain after Rooklet exits.
Privileged helpers own individual tool deadlines and cleanup. They finish accepted
incoming changes/readback or PF transactions/restoration before the supervising
process reports output or input errors. If the backend worker stops unexpectedly,
Rooklet restores the terminal and exits; unfinished operation outcomes are unknown.

## Optional network rules

Incoming application permissions work independently of PF. To use Rooklet's
machine-wide network rules, explicitly install its dedicated anchor:

```sh
rooklet network setup
rooklet network add 203.0.113.0/24 --name 'Example destination' --action block --direction out
rooklet network add any --port 8080 --protocol tcp --direction in --name 'Incoming example'
rooklet network status
```

These commands change network policy and may affect connectivity. Setup accepts
Apple's stock parent PF layout and refuses unsupported custom parent rules.
Apply also rejects changes to the supported live parent layout. PF is an advanced
administration mechanism that Apple does not consider a supported product API;
macOS updates and other networking software can affect compatibility. See
[PF scope and compatibility](docs/network.md#scope-and-compatibility).

Rules match a remote peer: source IP for incoming traffic, destination IP for
outgoing traffic. The port is the destination service port: local for incoming,
remote for outgoing. The first matching enabled rule inside Rooklet's anchor wins.

Press `w` in Network to explore a hypothetical connection. Enter its remote IP,
TCP/UDP protocol, inbound/outbound direction, destination service port, and
interface. Results update while you edit. A blank port or interface stays unknown;
if an earlier rule might depend on that field, Rooklet reports an undetermined
result instead of claiming a later rule wins. This predicts matching inside
Rooklet's anchor, not effective enforcement across other anchors or existing states.

Read-only CLI diagnostics use saved rules by default, or a JSON rule array supplied
as a file or with `--stdin`:

```sh
rooklet network check rules.json
rooklet network explain rules.json --remote 203.0.113.5 --protocol tcp --direction out --port 443 --interface en0
```

Reading saved rules may require prior `sudo -v`; supplied files need no privileges.
Checks report rules fully shadowed by a single earlier enabled rule. TUI reviews
show these warnings before applying additions, edits, toggles, deletions, or
reordering; CLI changes and profile checks/apply emit warnings on stderr.
Partial overlap and coverage by multiple earlier rules are not diagnosed.

Rooklet preserves other anchors and existing connection states. Established
connections may therefore continue after a rule changes. To clear Rooklet's rules
and release its PF enable reference, or remove the setup:

```sh
rooklet network disable
rooklet network remove
```

Another service may keep PF enabled. Rules are not automatically reapplied at
boot. See [PF configuration and lifecycle](docs/network.md) for JSON rule files,
read-only previews, backups, drift checks, persistence, and restoration limits.

## Countries and traffic

Country lookup is optional. Install the managed database once:

```sh
rooklet geoip update
```

Or press `g` in Settings / click **Update countries**. This explicitly downloads
[DB-IP Country Lite](https://db-ip.com/db/download/ip-to-country-lite), validates
it, and atomically installs it at
`~/Library/Application Support/rooklet/geoip/Country.mmdb`. No account, API key,
or sudo is needed. Later launches load it automatically; a running TUI also
picks up replacements.

Lookups stay offline and cached. Observed endpoint addresses are never sent to
the provider. Downloads happen only when requested, and failed updates retain
the previous database. Settings shows attribution and database age. The Lite
database is updated monthly and has reduced coverage and accuracy.

IP geolocation by [DB-IP](https://db-ip.com/), licensed under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Rooklet uses the
unmodified database. Country labels estimate the observed IP's location; CDNs,
VPNs, proxies, and anycast can make that different from an app's actual service
location. Local/private peers show `Local`; missing data shows `Unknown`.

Activity rows show peer counts, country summaries, download/upload rates, and
received/sent totals without selecting an app. Expand an app to see its peers;
`Enter` on a peer opens the full endpoint. Wider windows add incoming permissions
and application paths. At 120 columns or wider, resource columns replace the path
with captured process count, CPU, and estimated memory footprint. Press `r` or
click **Resources** to hide these columns and restore the path.

Press `i` or click **Details** to inspect the selected app's captured processes:
PID, owner UID, parent PID, start time, executable path, CPU, memory, and disk
read/write rates. Scroll with ↑↓, Page Up/Down, Home/End, or the mouse wheel.
The inspector also works in narrow terminals; peer details remain available.

Resource counters use native macOS APIs, without additional monitoring processes
or administrator authentication. Collection runs every two seconds while the
columns, app inspector, or resource sort are active. Switching views, hiding
resources, or freezing Activity stops counter collection. Process discovery runs
every five seconds and caches verified identities and paths. Each observation
has a cooperative 20 ms collection budget and a 4096-process limit; individual OS
calls may exceed that budget. Discovery and sampling continue across observations
when necessary, with visible apps prioritized and fair rotation for other apps.

CPU 100% means one occupied core; multi-core apps can exceed 100%. Memory is the
sum of captured helpers' physical footprints, an estimate rather than unique
system-wide memory. Disk rates describe observed process disk I/O, separately
from network traffic. Helpers without network traffic contribute to resource
totals when their verified bundle matches the observed app. Resource observation
covers the current user's accessible processes. Counts reflect captured members.

Memory appears after the first successful sample; CPU and disk rates need two
observations. `—` means warming up or unavailable;
`~` marks partial or stale values. Details show sample age and coverage. CPU and
memory sorting places complete fresh readings first; incomplete readings remain
explicit. PID reuse, executable changes, failed reads, and resumed sampling reset
rate baselines. Resource data is observational and never supplies a firewall verdict.

Routine firewall observations are cached for up to five seconds, independently
of traffic polling. Authentication refresh, mutations, and profile preparation
force fresh control reads. JSON status includes the control observation's age.

Press `s` or click the Activity table's **Sort** title to cycle observed order,
download/upload rates, received/sent totals, app name, peer count, CPU, and memory. Numeric
sorts put the largest values first; selection follows the same app or peer
as the list moves.

Activity search accepts ordinary text and structured filters joined with AND:

| Filter | Meaning |
| --- | --- |
| `app:Safari` | App name or path contains the text |
| `country:US`, `country:"United States"` | Country code or full name; also `local` / `unknown` |
| `proto:tcp`, `proto:udp`, `proto:any` | Protocol of an observed peer |
| `incoming:allow` | Registered incoming state: `allow`, `block`, `mixed`, `unregistered`, `unavailable` |
| `scope:local`, `scope:public` | Local/private or public observed address |
| `ip:203.0.113.0/24` | Remote IPv4/IPv6 address or CIDR |
| `port:443` | Observed remote port |

For example, `app:Safari country:US proto:tcp port:443` shows matching peers and
their parent apps, automatically expanding the matching peers. All peer filters
must match the same peer. Parent totals still represent the entire app. Invalid
filter fields or values show an error.

Select an expanded peer and press `n` to draft a machine-wide rule with its IP,
observed protocol, and known remote port. The initial direction is an **outbound
proposal**, because observations do not identify connection direction. Review
the editable fields and confirmation before applying; inbound rules use the
local service port. The rule affects every app, not just the selected app.

Rates average traffic over the elapsed time between observation reads, including
samples buffered during backend work. Totals start at the first observation
and preserve contributions across helper exits. Process totals are authoritative;
child-flow totals are not added again. Peer counts represent observed
endpoint/port/protocol combinations, not socket counts. Peer rates are unavailable.
Short connections, hidden processes, and wildcard UDP peers may be absent.
Helpers are grouped by verified app paths; unresolved processes stay separate.

## Keyboard and mouse

| Key | Action |
| --- | --- |
| `Tab`, `1`–`4` | Switch views |
| `↑↓`, `j` / `k` | Move selection |
| `Enter` | Expand peers, inspect a peer, edit a network rule, or review a setting |
| `a` / `b` | Review incoming allow/block permissions |
| `n` | Add an app, draft a rule from a selected Activity peer, or add a Network rule |
| `s` in Activity | Cycle sorting; click the table's Sort title for the same action |
| `w` in Network | Explain a hypothetical connection against saved Rooklet rules |
| `d` | Remove a registered app entry or network rule |
| `t`, `+` / `-` | Toggle or reorder a network rule |
| `/`, `Esc` | Search; clear search or cancel a dialog |
| `Space` in Activity | Freeze traffic display; firewall status still refreshes |
| `x` / `X` in Activity | Review termination / force kill of the app and captured helpers |
| `u` | Authenticate for privileged reads and changes |
| `g` in Settings | Install/update country data |
| `p` | Open saved profiles; review changes or export the current configuration |
| `?`, `q`, `Ctrl-C` | Help; quit |

Click tabs, rows, footer shortcuts, and dialog buttons. Double-click a row to
expand activity, inspect a peer, edit a rule, or review an app permission or
setting. Click Activity's expand indicator once to expand/collapse. The mouse
wheel moves selection over lists and scrolls confirmation text. In the network
editor and explanation dialog, click text fields to focus and choice fields to cycle values.

Mouse navigation needs a terminal with mouse reporting. Capture is suspended
during authentication and released when Rooklet exits.

`x` requests `SIGTERM`, which an app can ignore. `X` requests `SIGKILL` through a
separate confirmation. Both list every captured PID and executable path, including
helpers without traffic. Unsaved work may be lost, and apps may restart.

Termination needs no sudo for your own processes. Rooklet rechecks ownership and
kernel process identity before signaling, protects root-owned processes, itself,
and ancestors, and refuses stale targets. If identity-bound signaling is
unavailable on the host, termination fails explicitly. Signal delivery does not
guarantee that a process has exited.

## Profiles

Press `p` from any view to open saved profiles. Select a profile with the arrow
keys or mouse, then press `Enter` / click **Review** to inspect changes to incoming
settings, app permissions, and ordered machine-wide PF rules. Applying requires a
separate confirmation and normal administrator authentication. A changed firewall
baseline invalidates the review; reopen it before applying again.

In the profile list, press `e` / click **Export** and enter a new name to save the
current configuration. Existing profiles are never overwritten. Complete firewall
status is required; close the dialog and press `u` to unlock privileged reads if
needed. Profiles are stored as JSON in
`~/Library/Application Support/rooklet/profiles/`. Names use up to 64 ASCII letters,
digits, spaces, underscores, or hyphens. Existing CLI profile files can be placed
in this directory with a valid name and `.json` extension.

Profiles replace all three included scopes, including removal of incoming app
entries and PF rules absent from the profile. Applying a profile with a configured
PF setup applies its ordered rules. The review describes any activation changes
and shadow warnings. Restoration is best effort; ALF changes are not atomic.
No automatic network switching or startup application is performed.

Before applying, Rooklet also checks the proposed PF rules against the host's
interfaces, managed parent layout, trusted files, and PF syntax without changing
firewall state. The subsequent application still revalidates live state.

The same validation and application logic is available from the CLI:

```sh
rooklet profile export profile.json
rooklet profile check profile.json
rooklet profile apply profile.json --yes
```

Exports contain incoming settings, registered app entries, and saved network
rules. Run `sudo -v` in the same terminal before export so PF state can be read.
Export refuses to overwrite a file. Check validates the strict schema without
applying it. Unknown fields and unsupported formats are rejected.

Apply replaces those scopes, resolves bundle paths before mutation, and rejects
duplicate resolved registrations. Failures trigger best-effort restoration and
report restoration failures explicitly. Profiles containing network rules
require prior PF setup. Review exported files before sharing: they contain app
paths and network policy.

## Diagnostic logging

Logging is optional and writes JSONL files without changing terminal output:

```sh
rooklet --log-dir "$HOME/Library/Logs/rooklet"
rooklet --log-dir "$HOME/Library/Logs/rooklet" --log-level debug status
```

The directory is created private to the current user. An existing directory must
be private and owned by that user; symlink directories and log files are refused.
`--log-level` requires `--log-dir` and accepts `error`, `warn`, `info` (default),
`debug`, and `trace`.

Each run has a file capped at 4 MiB; startup retains the five newest run files.
Writing uses a bounded queue so disk I/O does not block firewall operations or
rendering. A final record reports dropped events and write failures when the
sink remains writable. Shutdown drains logs after terminal restoration and
backend completion, with a one-second limit for the log writer.

Logs record operation categories, outcomes, phases, durations, and counts. They
exclude application paths, names, endpoints, profile contents, command arguments,
and command output at every level. Privileged helper processes do not inherit
logging options. Diagnostics are best effort, not an audit trail; review files
before sharing. See
[ADR 0007's event map and logging contract](docs/adr/0007-private-bounded-diagnostics.md#event-map).

## Development and packaging

The Cargo workspace contains three packages with one distributed executable:

| Package | Responsibility |
| --- | --- |
| `rooklet-core` | Platform-free models, captured evidence, schemas, validation, PF compilation, and rule analysis |
| `rooklet-macos` | Firewall operations, process identity and signaling, traffic/resource collection, GeoIP, and profile storage/application |
| `rooklet` | CLI, authentication, TUI interaction state, workers, and rendering |

`rooklet-macos` depends on `rooklet-core`; the application depends on both.
Core never depends on the platform or UI packages. Parsers, subprocess runners,
and native collection are private implementation modules. Administrator
requests use typed operations and never prompt from worker subprocesses.
The internal libraries are workspace packages, not separately published APIs.

The [documentation index](docs/README.md) links the current
[architecture](docs/architecture.md), [functionality](docs/functional.md), and
[architectural decision records](docs/adr/README.md). Contribution and documentation
maintenance rules live in the root [AGENTS.md](AGENTS.md).

```sh
make check
make test
make release
make package
```

For a read-only native sampler timing diagnostic:

```sh
cargo test -p rooklet-macos --release --locked resources::tests::native_observation_cost_diagnostic -- --ignored --nocapture
```

Checks cover every workspace package: formatting, all-target compiler checks,
and Clippy with warnings as errors. Tests cover command formats, grouping,
counters, strict validation, configuration drift, cancellation, confirmations,
mouse geometry, and rendering.
Tests do not change the host firewall; PF syntax checks use `pfctl -n`, and native
signal tests target only processes they create. GitHub Actions runs the full
workspace and packaging on native Apple silicon and Intel macOS 15 runners,
portable core checks on Linux, and a full Rust 1.88 compiler check.
Matching version tags publish the verified binary packages to GitHub Releases;
see [build and release instructions](docs/releases.md).

`make release` builds the Rust toolchain's native macOS target explicitly and
prints the executable path under `target/<target-triple>/release/rooklet`.
`make install` builds and installs that binary into `~/.local/bin`;
`make uninstall` removes it. Override the destination with
`make install BIN_DIR="/absolute/path/to/bin"` and the same value for uninstall.
These Make targets require Rust; the scripts in a binary archive do not.

`make package` creates an architecture-verified `.tar.gz` and matching `.sha256`
file in `target/package/`. The archive contains the executable, install/uninstall
scripts, license, and a short installation guide as `README.md`. Screenshots and
development documentation stay in the repository.
It uses fresh staging and excludes macOS resource forks and extended metadata.
Cargo's configured target directory or `CARGO_BUILD_TARGET` cannot cause a stale
binary to be packaged. No signing account, provisioning, or notarization is used.

An explicit supported target can be selected when its Rust standard library and
macOS build tools are installed:

```sh
make package TARGET=aarch64-apple-darwin
make package TARGET=x86_64-apple-darwin
```

The package version comes from Cargo metadata; packaging never needs to run a
cross-built executable. Archives are separate builds for each architecture,
rather than a universal binary. Cross-built Intel releases still need runtime
testing on an Intel Mac.

Contributions are welcome. Follow [AGENTS.md](AGENTS.md), run the checks above,
and keep live firewall integration tests in an isolated environment. Bug reports
should include the macOS version, terminal, Rooklet version, and relevant
`rooklet doctor` output. Redact app paths, addresses, and other private information.

## License

Rooklet's source code is [MIT licensed](LICENSE). The optional DB-IP country data
has its own [CC BY 4.0 license](https://creativecommons.org/licenses/by/4.0/).
The synthetic MaxMind database used in tests is Apache 2.0 licensed; its provenance
and license are retained in [GeoIP test fixtures](crates/rooklet-macos/tests/data/README.md). Production country
data is downloaded explicitly and is not bundled in this repository or packages.
