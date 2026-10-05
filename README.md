# Xield

A compact Rust TUI for macOS firewall settings, network rules, and app traffic.
Manage incoming permissions, inspect connections and countries, and apply
machine-wide network rules from one terminal.

One executable. Keyboard and mouse navigation. No Swift companion, system
extension, Apple Developer account, or notarization workflow for source installs.

## Quick start

Requires **macOS**, **Rust 1.88+**, and the macOS command-line build tools.
Clone this repository, open its directory, and install from source:

```sh
cargo install --path . --locked
```

Run as your normal user:

```sh
xield doctor
xield
```

Launching Xield does not activate or change either firewall. Administrator
access is requested when needed through normal `sudo` authentication; you do
not need to run the whole app with `sudo`.

Without installing, use `cargo run --release --locked --`. Use at least **80 × 24**
for comfortable navigation; the minimum is 50 × 17. No patched font is required.

## Binary installation (no Rust required)

Each archive includes a [short installation guide](docs/install.md), the binary,
install/uninstall scripts, and the license.

Use the archive for your Mac: `macos-arm64` for Apple silicon, or
`macos-x86_64` for Intel. With the archive and its matching `.sha256` file in the
same directory, verify and extract it. For version 0.2.0 on Apple silicon:

```sh
shasum -a 256 -c xield-0.2.0-macos-arm64.tar.gz.sha256
tar -xzf xield-0.2.0-macos-arm64.tar.gz
cd xield-0.2.0-macos-arm64
sh install.sh
export PATH="$HOME/.local/bin:$PATH"
xield --version
```

The bundled installer needs only macOS's standard tools; neither Rust nor Xcode
command-line tools are needed. It copies the executable
to `~/.local/bin/xield`, sets executable permissions, and atomically replaces an
existing regular file for upgrades. It does not request sudo, run Xield, edit
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

Uninstall removes only the `xield` executable. Applied firewall settings, PF
rules, backups, and country data remain. If you want to remove Xield's PF setup,
explicitly run `xield network remove` before uninstalling. For an installation
made with `cargo install`, use `cargo uninstall xield` instead.

## Features and scope

| View | What you can do | Backend and scope |
| --- | --- | --- |
| Activity | View app and helper traffic, peers, countries, rates, and totals; inspect peers; review process termination | Observed activity from macOS `nettop` |
| Applications | Add, allow, block, and remove registered incoming permissions | macOS application firewall (`socketfilterfw`) |
| Network | Add, edit, reorder, and toggle IP/CIDR, port, protocol, direction, and interface rules | PF rules for every application on the Mac |
| Settings | Turn the incoming firewall on/off; manage stealth, block-all, signed-app defaults, and country data | macOS application firewall and local GeoIP database |

Xield also provides JSON status, configuration profiles, rule previews/explanations, and
light/monochrome themes:

```sh
xield status
xield --theme light
xield --theme mono
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
xield firewall status
xield apps list
xield apps add /Applications/Example.app
xield apps block /Applications/Example.app
xield apps allow /Applications/Example.app
xield apps remove /Applications/Example.app
xield firewall set firewall on
xield firewall set stealth on
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
Page Up/Down, Home/End, or the mouse wheel to review every target. Xield validates
targets before changing them and verifies each result. Application firewall
operations are sequential; partial failures identify affected entries.
Failed operations trigger a status readback. If that readback is unavailable,
cached firewall controls become unavailable until a successful refresh.

Authentication happens outside terminal raw mode. Xield never collects or stores
passwords, and worker commands use noninteractive `sudo`. Quitting waits for an
authorized mutation to finish. Applied settings remain after Xield exits.
PF helpers retain individual tool deadlines and finish any restoration before
the supervising process reports output or input errors.

## Optional network rules

Incoming application permissions work independently of PF. To use Xield's
machine-wide network rules, explicitly install its dedicated anchor:

```sh
xield network setup
xield network add 203.0.113.0/24 --name 'Example destination' --action block --direction out
xield network add any --port 8080 --protocol tcp --direction in --name 'Incoming example'
xield network status
```

These commands change network policy and may affect connectivity. Setup accepts
Apple's stock parent PF layout and refuses unsupported custom parent rules.
Rules match a remote peer: source IP for incoming traffic, destination IP for
outgoing traffic. The port is the destination service port: local for incoming,
remote for outgoing. The first matching enabled rule inside Xield's anchor wins.

Press `w` in Network to explore a hypothetical connection. Enter its remote IP,
TCP/UDP protocol, inbound/outbound direction, destination service port, and
interface. Results update while you edit. A blank port or interface stays unknown;
if an earlier rule might depend on that field, Xield reports an undetermined
result instead of claiming a later rule wins. This predicts matching inside
Xield's anchor, not effective enforcement across other anchors or existing states.

Read-only CLI diagnostics use saved rules by default, or a JSON rule array supplied
as a file or with `--stdin`:

```sh
xield network check rules.json
xield network explain rules.json --remote 203.0.113.5 --protocol tcp --direction out --port 443 --interface en0
```

Reading saved rules may require prior `sudo -v`; supplied files need no privileges.
Checks report rules fully shadowed by a single earlier enabled rule. TUI reviews
show these warnings before applying additions, edits, toggles, deletions, or
reordering; CLI changes and profile checks/apply emit warnings on stderr.
Partial overlap and coverage by multiple earlier rules are not diagnosed.

Xield preserves other anchors and existing connection states. Established
connections may therefore continue after a rule changes. To clear Xield's rules
and release its PF enable reference, or remove the setup:

```sh
xield network disable
xield network remove
```

Another service may keep PF enabled. Rules are not automatically reapplied at
boot. See [PF configuration and lifecycle](docs/network.md) for JSON rule files,
read-only previews, backups, drift checks, persistence, and restoration limits.

## Countries and traffic

Country lookup is optional. Install the managed database once:

```sh
xield geoip update
```

Or press `g` in Settings / click **Update countries**. This explicitly downloads
[DB-IP Country Lite](https://db-ip.com/db/download/ip-to-country-lite), validates
it, and atomically installs it at
`~/Library/Application Support/xield/geoip/Country.mmdb`. No account, API key,
or sudo is needed. Later launches load it automatically; a running TUI also
picks up replacements.

Lookups stay offline and cached. Observed endpoint addresses are never sent to
the provider. Downloads happen only when requested, and failed updates retain
the previous database. Settings shows attribution and database age. The Lite
database is updated monthly and has reduced coverage and accuracy.

IP geolocation by [DB-IP](https://db-ip.com/), licensed under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Xield uses the
unmodified database. Country labels estimate the observed IP's location; CDNs,
VPNs, proxies, and anycast can make that different from an app's actual service
location. Local/private peers show `Local`; missing data shows `Unknown`.

Activity rows show peer counts, country summaries, download/upload rates, and
received/sent totals without selecting an app. Expand an app to see its peers;
`Enter` on a peer opens the full endpoint. Wider windows add incoming permissions
and application paths.

Press `s` or click the Activity table's **Sort** title to cycle observed order,
download/upload rates, received/sent totals, app name, and peer count. Numeric
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
| `w` in Network | Explain a hypothetical connection against saved Xield rules |
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
during authentication and released when Xield exits.

`x` requests `SIGTERM`, which an app can ignore. `X` requests `SIGKILL` through a
separate confirmation. Both list every captured PID and executable path, including
helpers without traffic. Unsaved work may be lost, and apps may restart.

Termination needs no sudo for your own processes. Xield rechecks ownership and
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
`~/Library/Application Support/xield/profiles/`. Names use up to 64 ASCII letters,
digits, spaces, underscores, or hyphens. Existing CLI profile files can be placed
in this directory with a valid name and `.json` extension.

Profiles replace all three included scopes, including removal of incoming app
entries and PF rules absent from the profile. Applying a profile with a configured
PF setup applies its ordered rules. The review describes any activation changes
and shadow warnings. Restoration is best effort; ALF changes are not atomic.
No automatic network switching or startup application is performed.

Before applying, Xield also checks the proposed PF rules against the host's
interfaces, managed parent layout, trusted files, and PF syntax without changing
firewall state. The subsequent application still revalidates live state.

The same validation and application logic is available from the CLI:

```sh
xield profile export profile.json
xield profile check profile.json
xield profile apply profile.json --yes
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

## Development and packaging

```sh
make check
make test
make release
make package
```

Checks run formatting, all-target compiler checks, and Clippy with warnings as
errors. Tests cover command formats, grouping, counters, strict validation,
configuration drift, cancellation, confirmations, mouse geometry, and rendering.
Tests do not change the host firewall; PF syntax checks use `pfctl -n`, and native
signal tests target only processes they create. GitHub Actions runs checks on macOS.

`make release` builds the Rust toolchain's native macOS target explicitly and
prints the executable path under `target/<target-triple>/release/xield`.
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
should include the macOS version, terminal, Xield version, and relevant
`xield doctor` output. Redact app paths, addresses, and other private information.

## License

Xield's source code is [MIT licensed](LICENSE). The optional DB-IP country data
has its own [CC BY 4.0 license](https://creativecommons.org/licenses/by/4.0/).
The synthetic MaxMind database used in tests is Apache 2.0 licensed; its provenance
and license are retained in [tests/data](tests/data/README.md). Production country
data is downloaded explicitly and is not bundled in this repository or packages.
