#!/usr/bin/env bash
# Lint the same target, binary and composition as the firmware build command.
set -euo pipefail
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
artifact_dir="${FIRMWARE_CARGO_TARGET_DIR:-$repo_root/target}"
[[ "$artifact_dir" == /* ]] || artifact_dir="$repo_root/$artifact_dir"
export FIRMWARE_CARGO_TARGET_DIR="$artifact_dir"
# shellcheck source=../lib/inkplate_cargo.sh
source "$repo_root/scripts/lib/inkplate_cargo.sh"
inkplate_cargo clippy "$@"
