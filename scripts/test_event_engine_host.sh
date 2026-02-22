#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
toolchain="${RUSTUP_TOOLCHAIN:-stable}"

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

(
  cd /tmp
  RUSTUP_TOOLCHAIN="$toolchain" cargo test \
    --locked \
    --manifest-path "$repo_root/tools/event_engine_host_harness/Cargo.toml" \
    --target "$host_target" \
    "$@"
)
