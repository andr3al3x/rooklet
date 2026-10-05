#!/bin/sh
set -eu

for argument in "$@"; do
    case "$argument" in
        -h|--help)
            printf '%s\n' 'Usage: package.sh [--target aarch64-apple-darwin|x86_64-apple-darwin]' \
                'Builds a verified binary archive and SHA-256 file under target/package/.'
            exit 0
            ;;
    esac
done
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
binary_path=$(sh "$project_dir/scripts/build-release.sh" "$@")
package_id=$(cargo pkgid --manifest-path "$project_dir/Cargo.toml" --locked)
release_version=${package_id##*#}
release_version=${release_version##*@}
case "$release_version" in ''|*[!0-9A-Za-z.+-]*) printf '%s\n' 'invalid release version' >&2; exit 1 ;; esac
release_arch=$(/usr/bin/lipo -archs "$binary_path")
package_name="xield-$release_version-macos-$release_arch"
archive_name="$package_name.tar.gz"
output_dir="$project_dir/target/package"
mkdir -p "$output_dir"
staging_dir=$(mktemp -d "$output_dir/.xield-package.XXXXXX")
trap 'rm -rf "$staging_dir"' EXIT
trap 'exit 1' HUP INT TERM
package_dir="$staging_dir/$package_name"
mkdir -p "$package_dir"
cp "$binary_path" "$package_dir/xield"
cp "$project_dir/LICENSE" "$package_dir/LICENSE"
cp "$project_dir/docs/install.md" "$package_dir/README.md"
cp "$project_dir/scripts/install.sh" "$package_dir/install.sh"
cp "$project_dir/scripts/uninstall.sh" "$package_dir/uninstall.sh"
# Exclude macOS resource forks and extended metadata from the public archive.
COPYFILE_DISABLE=1 tar -czf "$staging_dir/$archive_name" -C "$staging_dir" "$package_name"
(cd "$staging_dir" && /usr/bin/shasum -a 256 "$archive_name" > "$archive_name.sha256")
mv "$staging_dir/$archive_name" "$output_dir/$archive_name"
mv "$staging_dir/$archive_name.sha256" "$output_dir/$archive_name.sha256"
printf '%s\n' "$output_dir/$archive_name" "$output_dir/$archive_name.sha256"
