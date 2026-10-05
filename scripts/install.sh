#!/bin/sh
# Also distributed as install.sh beside the packaged xield executable.
set -eu

fail() { printf '%s\n' "xield: $*" >&2; exit 1; }
usage() {
    printf '%s\n' 'Usage: install.sh [--bin-dir DIR] [--binary FILE]' \
        'Default destination: $HOME/.local/bin. No sudo or shell configuration changes.'
}
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
binary_path="$script_dir/xield"
bin_dir=''
while [ "$#" -gt 0 ]; do
    case "$1" in
        --bin-dir|--binary)
            [ "$#" -ge 2 ] && [ -n "$2" ] || fail "$1 needs a nonempty path"
            if [ "$1" = --bin-dir ]; then bin_dir=$2; else binary_path=$2; fi
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
case "$binary_path" in /*) ;; *) binary_path="$PWD/$binary_path" ;; esac
[ -f "$binary_path" ] && [ -x "$binary_path" ] || fail "missing executable: $binary_path; use the release archive or make install"
# file is shipped with macOS; lipo/otool are developer-tool shims and must not
# make a downloaded binary require Xcode's command-line tools.
binary_description=$(/usr/bin/file -bL "$binary_path")
case "$binary_description" in
    'Mach-O 64-bit executable arm64'|'Mach-O 64-bit executable arm64, '*) binary_arch=arm64 ;;
    'Mach-O 64-bit executable x86_64'|'Mach-O 64-bit executable x86_64, '*) binary_arch=x86_64 ;;
    *) fail 'source must be a thin arm64 or x86_64 macOS executable' ;;
esac
host_arch=$(/usr/bin/uname -m)
compatible_arches=$host_arch
# A Rosetta terminal can run its Intel binary or launch a native arm64 binary.
if [ "$host_arch" = x86_64 ] && [ "$(/usr/sbin/sysctl -n sysctl.proc_translated 2>/dev/null || true)" = 1 ]; then
    compatible_arches="arm64 x86_64"
fi
case " $compatible_arches " in
    *" $binary_arch "*) ;;
    *) fail "binary architecture ($binary_arch) does not match this Mac (compatible: $compatible_arches)" ;;
esac
destination="$bin_dir/xield"
[ ! -L "$destination" ] || fail "refusing to replace a symlink: $destination"
[ ! -e "$destination" ] || [ -f "$destination" ] || fail "destination is not a regular file: $destination"
mkdir -p "$bin_dir" || fail "cannot create $bin_dir; choose a writable --bin-dir"
staged_binary=$(mktemp "$bin_dir/.xield-install.XXXXXX") || fail "cannot write $bin_dir; choose a writable --bin-dir"
trap 'rm -f "$staged_binary"' EXIT
trap 'exit 1' HUP INT TERM
# Preserve extended attributes, including quarantine; never bypass Gatekeeper.
/bin/cp "$binary_path" "$staged_binary"
/bin/chmod 755 "$staged_binary"
/bin/mv -f "$staged_binary" "$destination"
printf '%s\n' "Installed $destination" 'Firewall settings and country data are unchanged.'
case ":${PATH-}:" in
    *":$bin_dir:"*) ;;
    *) printf '%s\n' "Add $bin_dir to PATH in your shell configuration, or run $destination directly." ;;
esac
