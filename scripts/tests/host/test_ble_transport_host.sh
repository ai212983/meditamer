#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"
toolchain="${RUSTUP_TOOLCHAIN:-stable}"
host_target="$(rustup run "$toolchain" rustc -vV | awk '/^host:/ {print $2}')"
target_dir="$(mktemp -d)"
trap 'rm -r "$target_dir"' EXIT

if [[ -z "$host_target" ]]; then
  echo "could not determine host target triple" >&2
  exit 1
fi

(
  cd /tmp
  CARGO_TARGET_DIR="$target_dir" RUSTUP_TOOLCHAIN="$toolchain" cargo test \
    --locked \
    --manifest-path "$repo_root/tools/ble_transport_host_harness/Cargo.toml" \
    --target "$host_target"
)
