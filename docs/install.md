# Xield binary installation

Xield is a terminal application for macOS firewall management and network activity.
This archive contains the executable, install/uninstall scripts, and MIT license.
Rust and Xcode command-line tools are not required to install it.

Choose `macos-arm64` for Apple silicon or `macos-x86_64` for Intel. Download the
archive and its matching `.sha256` file together. For version 0.2.0 on Apple silicon:

```sh
shasum -a 256 -c xield-0.2.0-macos-arm64.tar.gz.sha256
tar -xzf xield-0.2.0-macos-arm64.tar.gz
cd xield-0.2.0-macos-arm64
sh install.sh
export PATH="$HOME/.local/bin:$PATH"
xield --version
```

The installer copies the binary to `~/.local/bin/xield`. Add that directory to
your shell's PATH for future sessions. To choose another writable directory:

```sh
sh install.sh --bin-dir /absolute/path/to/bin
```

Run the installer from a newer archive to upgrade. It atomically replaces an
existing regular executable and rejects destination symlinks or directories.
Installation does not request sudo, edit shell configuration, or change firewall
settings or country data. `sh install.sh --help` lists all options.

Release binaries have no Developer ID signature or notarization. Gatekeeper may
block downloaded software; follow [Apple's guidance](https://support.apple.com/en-us/102445).
The installer preserves quarantine attributes.

## Run

Run as your normal user:

```sh
xield doctor
xield
```

Launching Xield does not enable or change either firewall. Operations that need
administrator access use normal sudo authentication; the whole app does not need
sudo. Use an 80 × 24 or larger terminal. Keyboard and mouse navigation are supported.
Use `?` for TUI help and `xield --help` for CLI commands.

Country labels are optional. Explicitly run `xield geoip update` to download the
DB-IP Country Lite database; subsequent lookups stay offline. Incoming app
permissions use macOS's application firewall. PF network rules are machine-wide.
Observed traffic is not a firewall verdict or a complete log of blocked attempts.

## Uninstall

From an extracted archive:

```sh
sh uninstall.sh
# For a custom installation directory:
sh uninstall.sh --bin-dir /absolute/path/to/bin
```

Only the executable is removed. Firewall settings, PF rules, backups, and country
data remain. To remove Xield's PF setup too, explicitly run `xield network remove`
before uninstalling. Review `xield network --help` before changing network policy.
