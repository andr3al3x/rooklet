# Functionality

Rooklet provides incoming firewall management, machine-wide PF rules, and local
activity inspection through a keyboard-first TUI with mouse navigation and an
explicit CLI. This document describes supported behavior, not a roadmap. See the
[README](../README.md) for detailed keys and command examples, the
[architecture](architecture.md) for implementation boundaries, and
[ADRs](adr/README.md) for design decisions.

## Startup and access

- Launching `rooklet` starts the TUI without enabling or changing either firewall.
- Run as your normal user. Administrator operations use normal sudo authentication;
  the whole application does not need sudo. Privileged reads may require `u` in the
  TUI or prior `sudo -v` for CLI commands that read protected PF state.
- Without an interactive terminal, use explicit commands such as `rooklet status`.
- Dark, light, and monochrome themes are available without a patched font. The
  minimum layout is 50 × 17; 80 × 24 or larger is recommended.
- Keyboard and mouse actions share confirmation and authentication. Dialogs block
  underlying controls; mouse capture is suspended during authentication and
  released on exit.

## TUI capabilities

| View/workflow | Supported behavior | Scope and limits |
| --- | --- | --- |
| Activity | App/helper traffic, peer expansion and inspection, countries, rates/totals, filtering, sorting, frozen display | Sampled observations; short or inaccessible connections can be absent |
| Resource details | Captured process count, CPU, estimated memory, process identities, disk read/write rates, coverage and age | Accessible current-user processes grouped by verified app paths |
| Applications | Add, allow, block, or remove incoming registrations; review permissions from Activity when identity evidence permits | ALF incoming permissions; uncertain grouping cannot invent a registration |
| Network | Add/edit/toggle/reorder/delete rules, review shadow warnings, draft rules from peers, explain hypothetical matches | Machine-wide PF anchor; existing connection states are preserved |
| Settings | Toggle incoming firewall, stealth, block-all, signed-app defaults; explicitly install/update country data | ALF settings and local database; does not toggle every firewall on the Mac |
| Profiles | List named profiles, export to a new name, review all scopes, confirm application | Replaces incoming settings/registrations and configured PF rules; reviewed baseline must still match |
| Termination | Review `SIGTERM` or separately confirm `SIGKILL` for captured app/helper targets | Own eligible processes only; delivery is not proof of exit |

Quit waits for accepted operations to finish. An action failure and a failed
status refresh are reported separately; partial state or failed restoration is
not presented as success.

## Traffic, countries, and resources

App rows expose peer/country summaries, download/upload rates, and received/sent
totals without selection. Wider layouts add permission/path details; at 120
columns resource columns can show process count, CPU, and estimated memory.
`i` opens details, `r` toggles resources, and `s` cycles sorting. Selection follows
the same app or peer when rows reorder.

Search combines plain text with `app:`, `country:`, `proto:`, `incoming:`, `scope:`,
`ip:`, and `port:` filters using AND. Peer conditions must match the same peer;
app totals still describe the whole app. Invalid filters show an error. Freeze
retains traffic, resources, and evidence while current firewall controls refresh.

Traffic totals begin at first observation and survive helper exits within the
session. Process totals are not counted again through child flows. App rates
average the elapsed observation interval; peer rates are unavailable. Peer counts
are observed endpoint/port/protocol combinations, not socket counts. Observations
do not establish connection direction: a drafted peer rule starts as an editable
outbound proposal, affecting every app.

Resource collection runs only when displayed, inspected, or used for sorting.
Hiding resources, leaving Activity, or freezing it stops resource counters; it
does not stop the traffic monitor. CPU 100% represents one core, memory is an
estimated sum of captured physical footprints, and disk I/O is separate from
network traffic. Memory can appear after one sample; CPU/disk rates need two.
`—` represents missing/warming data and `~` marks partial/stale values. Reuse,
identity changes, failed reads, and resumed sampling reset rate baselines.

Country labels require an explicit `geoip update` or Settings action. The managed
DB-IP Country Lite database is loaded automatically after installation, and
running sessions detect updates. Updates need no account, API key, or sudo;
failed updates retain valid previous data. Lookups stay offline and never send
observed endpoint addresses to the provider. Local/private peers show `Local`,
missing data shows `Unknown`, and public labels are geographic estimates. DB-IP
attribution and licensing remain visible.

## Firewall and profile behavior

Incoming settings and registrations are independent of PF rules. Blocking an
incoming app does not block its outgoing connections. Allowing it does not bypass
PF or another firewall. Some system-managed ALF entries can be protected by macOS.

Network rules support remote IP/CIDR or `any`, destination service port, protocol,
direction, interface, enabled state, and explicit order. A port requires TCP or
UDP. The service port is local inbound and remote outbound. Preview/analysis of
supplied arrays is read-only and requires no system access. Shadow warnings cover
complete coverage by one earlier rule; hypothetical explanations retain unknown
fields rather than claiming a live verdict.

PF setup is explicit and supports only validated parent layouts. Apply changes
Rooklet's anchor. Disable clears that anchor and releases only Rooklet's enable
reference, retaining metadata for reapplication; remove additionally removes the
managed setup/state. Unrelated anchors, rules, states, and references are
preserved. Unsupported layouts, drift, and unsafe managed files cause refusal or
explicit diagnostics. See the [PF guide](network.md).

Profiles contain incoming settings, registered applications, and ordered network
rules. Export requires complete control state and refuses overwrite. Check is
read-only and rejects unknown fields and unsupported formats. Apply can remove
entries/rules omitted from a profile, resolves registration paths, rejects
duplicates, rechecks the reviewed baseline, and preflights before mutation.
Profiles containing network rules need prior PF setup. ALF steps are sequential;
failure triggers best-effort restoration with explicit restoration diagnostics.
There is no cross-backend atomicity or automatic profile activation.

Termination confirms the exact captured targets and signal, rechecks ownership
and identity, and protects root-owned processes, Rooklet, and its ancestors.
Stale requests and hosts without identity-bound signaling are refused. Running
Rooklet as root disables this feature. Apps can ignore `SIGTERM`, restart, or lose
unsaved work; delivered signals do not guarantee exit.

## CLI surface

Use `rooklet --help` and nested `--help` for argument syntax.

| Command family | Operations |
| --- | --- |
| `status`, `doctor` | JSON snapshot; human tool/setup/availability diagnostics |
| `apps` | `list`, `add`, `allow`, `block`, `remove` incoming registrations |
| `firewall` | `status`, `set` incoming settings |
| `network` | `status`, `check`, `explain`, `preview`, `preflight`, `setup`, `apply`, `add`, `delete`, `toggle`, `move`, `disable`, `remove` |
| `profile` | `export`, `check`, `apply` with explicit `--yes` |
| `geoip` | Explicit `update` of the managed country database |

`--theme` selects the TUI palette. Optional `--log-dir` and `--log-level` enable
private bounded JSONL diagnostics without changing normal output; level defaults
to `info`, and specifying it requires a directory. Sensitive configuration and
raw command/error payloads are excluded at every level. Logs can lose events on
overflow, I/O failure, or shutdown timeout. See
[the logging contract in ADR 0007](adr/0007-private-bounded-diagnostics.md#storage-and-shutdown-contract).

CLI configuration output preserves exact JSON values. Human errors/notices
sanitize terminal controls and bidi overrides. Validation failures occur before
mutation; privileged backend subprocesses never prompt for credentials.

## Distribution and current limits

The application ships as one architecture-specific binary with checksum,
installation/removal scripts, license, and a short guide. Source builds need
Rust 1.88+ and macOS command-line build tools; binary installation does not.
The installer preserves quarantine and changes only the executable. Uninstall
retains live firewall configuration and data; PF removal is a separate explicit
operation. See [install.md](install.md).

Outgoing per-app enforcement, interactive approval of every new connection,
packet capture, a complete blocked-attempt log, automatic boot reapplication,
automatic profile switching, and timed rule expiry are not implemented. There
are no demo observations, legacy aliases, schema migrations, extensions, or
notarization workflows.

The project is an early release. Privileged mutation, connectivity enforcement,
VPN coexistence, and reboot behavior need isolated integration testing. Intel
and older macOS runtime behavior remain unverified. Diagnostic logs and observed
traffic cannot replace those checks.
