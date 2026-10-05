#!/bin/sh
# Remove only the installed executable; firewall rules and data require explicit cleanup.
set -eu

fail() { printf '%s\n' "xield: $*" >&2; exit 1; }
usage() { printf '%s\n' 'Usage: uninstall.sh [--bin-dir DIR]' 'Default destination: $HOME/.local/bin. Removes only the xield executable.'; }
bin_dir=''
while [ "$#" -gt 0 ]; do
    case "$1" in
        --bin-dir)
            [ "$#" -ge 2 ] && [ -n "$2" ] || fail '--bin-dir needs a nonempty path'
            bin_dir=$2
            shift 2
            ;;
        -h|--help) usage; exit 0 ;;
        *) usage >&2; fail "unknown argument: $1" ;;
    esac
done
if [ -z "$bin_dir" ]; then
    bin_dir="${HOME:?HOME must be set for the default installation directory}/.local/bin"
fi
[ "$(/usr/bin/uname -s)" = Darwin ] || fail 'Xield requires macOS'
case "$bin_dir" in /*) ;; *) fail '--bin-dir must be an absolute path' ;; esac
destination="$bin_dir/xield"
[ ! -L "$destination" ] || fail "refusing to remove a symlink: $destination"
if [ ! -e "$destination" ]; then
    printf '%s\n' "No Xield executable at $destination"
    exit 0
fi
[ -f "$destination" ] || fail "destination is not a regular file: $destination"
/bin/rm "$destination"
printf '%s\n' "Removed $destination" 'Firewall settings, PF rules, backups, and country data are unchanged.'
