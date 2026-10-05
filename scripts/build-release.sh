#!/bin/sh
# Build and locate one verified macOS release binary, independent of Cargo defaults.
set -eu

fail() { printf '%s\n' "rooklet: $*" >&2; exit 1; }
usage() { printf '%s\n' 'Usage: build-release.sh [--target aarch64-apple-darwin|x86_64-apple-darwin]'; }
release_target=''
while [ "$#" -gt 0 ]; do
    case "$1" in
        --target)
            [ "$#" -ge 2 ] && [ -n "$2" ] || fail '--target needs a Rust target triple'
            release_target=$2
            shift 2
            ;;
        -h|--help) usage; exit 0 ;;
        *) usage >&2; fail "unknown argument: $1" ;;
    esac
done
[ "$(/usr/bin/uname -s)" = Darwin ] || fail 'release binaries must be built on macOS'
if [ -z "$release_target" ]; then
    release_target=$(rustc -vV | sed -n 's/^host: //p')
fi
case "$release_target" in
    aarch64-apple-darwin) expected_arch=arm64 ;;
    x86_64-apple-darwin) expected_arch=x86_64 ;;
    *) fail "unsupported release target: $release_target" ;;
esac
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cargo build --manifest-path "$project_dir/Cargo.toml" --target-dir "$project_dir/target" \
    --target "$release_target" --release --locked
binary_path="$project_dir/target/$release_target/release/rooklet"
[ -f "$binary_path" ] && [ -x "$binary_path" ] || fail 'release executable is missing'
actual_arch=$(/usr/bin/lipo -archs "$binary_path")
[ "$actual_arch" = "$expected_arch" ] || fail "expected $expected_arch, found $actual_arch"
/usr/bin/otool -hv "$binary_path" | awk '
    $1 == "MH_MAGIC_64" || $1 == "MH_MAGIC" { headers++; if ($5 != "EXECUTE") invalid = 1 }
    END { exit (headers == 0 || invalid) }
' || fail "release is not a macOS executable"
printf '%s\n' "$binary_path"
