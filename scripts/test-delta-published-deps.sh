#!/usr/bin/env bash
set -euo pipefail

# Test the actual package outside the workspace's [patch.crates-io]. Optional
# Cargo flags (for example --offline) are forwarded to each Cargo command.
repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
task_tmp_dir=$(mktemp -d)
trap 'rm -rf -- "$task_tmp_dir"' EXIT

cargo package --manifest-path "$repo_root/crates/delta/Cargo.toml" \
    --no-verify --allow-dirty --target-dir "$task_tmp_dir/target" "$@"
archives=("$task_tmp_dir"/target/package/loro-delta-*.crate)
tar -xzf "${archives[0]}" -C "$task_tmp_dir"
package_name=$(basename -- "${archives[0]}" .crate)
manifest="$task_tmp_dir/$package_name/Cargo.toml"

# Exercise the minimum supported registry version, not the in-tree fork or a
# newer release that could hide a missing fix in loro-delta itself.
cargo update --manifest-path "$manifest" -p generic-btree --precise 0.10.7 "$@"
cargo tree --manifest-path "$manifest" --depth 1 "$@"
cargo test --manifest-path "$manifest" --locked "$@"
