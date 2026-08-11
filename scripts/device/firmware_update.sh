#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/run_hostctl.sh
source "$script_dir/../lib/run_hostctl.sh"

if [[ "$#" -ne 2 ]]; then
    echo "usage: $0 <application.bin> <32-byte-signing-seed>" >&2
    exit 2
fi

repo_root="$(_run_hostctl_repo_root)"
image_path="$(_run_hostctl_abs_path "$repo_root" "$1")"
key_path="$(_run_hostctl_abs_path "$repo_root" "$2")"
args=(firmware-update --image "$image_path" --key "$key_path")
if [[ -n "${ESPFLASH_PORT:-}" ]]; then
    args+=(--port "$ESPFLASH_PORT")
fi
if [[ -n "${HOSTCTL_FIRMWARE_UPDATE_LOG_PATH:-}" ]]; then
    output_path="$(_run_hostctl_abs_path "$repo_root" "$HOSTCTL_FIRMWARE_UPDATE_LOG_PATH")"
    args+=(--output "$output_path")
fi
if [[ "${HOSTCTL_FIRMWARE_UPDATE_STAGE_ONLY:-0}" == "1" ]]; then
    args+=(--stage-only)
fi

run_hostctl "${args[@]}"
