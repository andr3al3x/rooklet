# PF network rules

Rooklet uses a dedicated `rooklet` PF anchor for machine-wide network rules.
Application firewall settings are independent and remain managed by macOS.

Rooklet manages only its own anchor and files. Other PF anchors, states, and
enable references are preserved.

## Explicit setup

`rooklet network setup` authenticates through sudo and installs an empty initial
ruleset by default. A JSON rule array can be supplied as a file or
with `--stdin`. Setup changes `/etc/pf.conf` only when its parent layout matches
Apple's stock anchor configuration. Existing/custom live parent rules cause a
clear refusal. Rooklet does not rewrite arbitrary PF configurations.

The managed block references `/etc/pf.anchors/rooklet`. Setup preserves original
configuration text and permissions, creates a timestamped
`/etc/pf.conf.rooklet-backup-*`, validates candidates with `pfctl -n`, and loads the
supported parent configuration. Failures attempt restoration and report the
remaining state explicitly.

Normal apply updates only Rooklet's anchor. It does not reload the parent ruleset.
No global PF rules/state flush or `pfctl -d` is used.

## Rule files and previews

Rules are strict JSON arrays, for example:

```json
[
  {
    "id": "example",
    "name": "Example destination",
    "action": "block",
    "destination": "203.0.113.0/24",
    "port": 443,
    "protocol": "tcp",
    "direction": "out",
    "interface": null,
    "enabled": true
  }
]
```

```sh
rooklet network preview rules.json
rooklet network check rules.json
rooklet network explain rules.json --remote 203.0.113.5 --protocol tcp --direction out --port 443 --interface en0
rooklet network apply rules.json
rooklet network toggle example
rooklet network move example -1
rooklet network delete example
```

`rooklet network preflight rules.json` validates the proposed rules against the
host's interfaces, trusted managed files, live parent layout, PF syntax, and saved
state size. It needs administrator authentication but changes no firewall state.
Profile application uses this check before changing incoming permissions.

A port requires TCP or UDP. `any` protocol without a port covers all IP protocols.
Direction is `in`, `out`, or `both`. Destination is a remote peer; the port is the
destination service port (local inbound, remote outbound). `any` destination
expands to IPv4 and IPv6 rules. CIDRs are canonicalized. Names are display metadata
and never interpolated into PF source. Rule IDs use 1–55 ASCII bytes of
letters, digits, `_`, or `-`; interfaces also have strict syntax.

`check` reports enabled rules completely covered by one earlier enabled rule;
partial overlaps and coverage by a combination of rules are not diagnosed.
Warnings are also included in TUI mutation reviews and emitted on stderr before
CLI network changes and profile application. Warnings do not invalidate a ruleset.

`explain` evaluates a hypothetical TCP/UDP connection against saved or supplied
ordered rules. It predicts only Rooklet's anchor matching, not a live firewall
verdict. Missing destination ports and interfaces remain unknown: a possible
earlier match prevents claiming that a later rule wins. The destination port
is local for inbound queries and remote for outbound queries. No match does not
imply allow; other anchors and established states still affect traffic.

Both commands use saved rules when no file/`--stdin` is supplied, which may require
prior `sudo -v`. Supplied rule arrays are analyzed without system access or
privileges. In the TUI, press `w` in Network to edit a hypothetical connection.

## Ownership, state, and shutdown

PF ownership is reference-counted. Rooklet records the token returned by `pfctl -E`
and releases only that token with `pfctl -X`. Another service may keep PF enabled
after Rooklet is disabled. Reboot-stale tokens are checked against current PF references.

Root-owned state is `/Library/Application Support/Rooklet/network.json`, in a 0700
directory; the file is 0600. Writes use bounded, atomic replacement. Mutations use
an advisory lock. Symlinks, unsafe ownership/permissions, changed managed blocks,
and inconsistent persistent/live rules are rejected or reported as drift.

`network disable` clears Rooklet's loaded and persistent anchor, retains saved rule
metadata for explicit reapplication, and releases its enable reference. Existing
PF connection states are retained. `network remove` additionally removes the
managed parent reference, anchor file, and saved state, after backing up and
validating the supported parent configuration. The private lock directory and
configuration backups may remain; no unrelated files are removed.

Rules are not automatically enabled/reapplied at boot. PF's current enable state,
other system components, and manual root reloads determine what is loaded. Read
`network status` after reboot before applying anything. If the saved parent anchor
is absent from the live ruleset, explicit setup can recover only a supported
layout with no orphaned live Rooklet rules. Supply the desired rule array to setup;
omitting it requests an empty ruleset.

## Meaning of status

The JSON field `rules_available` means saved
metadata could be read. `configured` describes the managed parent configuration.
`enabled` is PF's observed global enable state. `applied` additionally checks
managed parent order, persistent anchor content, and loaded rule snapshot.

These checks establish configuration consistency, not a proof that every packet
will receive the intended verdict. Existing states, earlier Apple rules, routing,
VPNs, and other firewalls can affect traffic. Validate actual connectivity in an
isolated environment before relying on a policy.
