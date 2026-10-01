#!/usr/bin/env bash

# Stable repository-level entry point for Meditamer Inkplate firmware builds.
# The target wrapper owns the Cargo working directory, features, linker flags,
# and toolchain setup; this compatibility layer only preserves the historical
# root target directory used by hostctl and the firmware CI/static gates.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
target_build="$repo_root/targets/meditamer-inkplate/build.sh"

if [[ ! -x "$target_build" ]]; then
    echo "Inkplate firmware build wrapper is missing or not executable: $target_build" >&2
    exit 1
fi

firmware_target_dir="${FIRMWARE_CARGO_TARGET_DIR:-$repo_root/target}"
if [[ "$firmware_target_dir" != /* ]]; then
    firmware_target_dir="$repo_root/$firmware_target_dir"
fi
export FIRMWARE_CARGO_TARGET_DIR="$firmware_target_dir"
exec "$target_build" "$@"
