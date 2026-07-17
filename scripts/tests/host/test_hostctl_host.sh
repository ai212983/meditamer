#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"
toolchain="${HOSTCTL_HOST_RUSTUP_TOOLCHAIN:-stable}"

if [[ "${1:-}" != "" && "${1:-}" != --* ]]; then
  host_target="$1"
  shift
else
  host_target="$(rustup run "$toolchain" rustc -vV | awk '/^host:/ {print $2}')"
fi
if [[ -z "$host_target" ]]; then
  echo "could not determine host target triple" >&2
  exit 1
fi
host_target_dir="$repo_root/target/host-tools/hostctl-tests/$host_target"

(
  cd /tmp
  env \
    -u RUSTUP_TOOLCHAIN \
    -u CARGO_BUILD_TARGET \
    -u CARGO_TARGET_DIR \
    -u CARGO_ENCODED_RUSTFLAGS \
    -u CARGO_UNSTABLE_BUILD_STD \
    -u RUSTFLAGS \
    -u RUSTDOCFLAGS \
    -u RUSTC_WRAPPER \
    -u RUSTC_WORKSPACE_WRAPPER \
    CARGO_TARGET_DIR="$host_target_dir" \
    rustup run "$toolchain" cargo test \
    --locked \
    --manifest-path "$repo_root/tools/hostctl/Cargo.toml" \
    --target "$host_target" \
    "$@"
)
