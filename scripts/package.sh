#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cargo build --manifest-path "$project_dir/Cargo.toml" --target-dir "$project_dir/target" --release --locked
release_version=$("$project_dir/target/release/xield" --version | cut -d ' ' -f 2)
release_arch=$(uname -m)
package_name="xield-$release_version-macos-$release_arch"
output_dir="$project_dir/target/package"
mkdir -p "$output_dir"
staging_dir=$(mktemp -d "$output_dir/.xield-package.XXXXXX")
trap 'rm -rf "$staging_dir"' EXIT
trap 'exit 1' HUP INT TERM
package_dir="$staging_dir/$package_name"
mkdir -p "$package_dir/docs/images"
cp "$project_dir/target/release/xield" "$project_dir/LICENSE" "$project_dir/README.md" "$project_dir/AGENTS.md" "$package_dir/"
cp "$project_dir/docs/network.md" "$package_dir/docs/"
cp "$project_dir/docs/images/activity.png" "$package_dir/docs/images/"
# Exclude macOS resource forks and extended metadata from the public archive.
COPYFILE_DISABLE=1 tar -czf "$staging_dir/archive.tar.gz" -C "$staging_dir" "$package_name"
mv "$staging_dir/archive.tar.gz" "$output_dir/$package_name.tar.gz"
echo "$output_dir/$package_name.tar.gz"
