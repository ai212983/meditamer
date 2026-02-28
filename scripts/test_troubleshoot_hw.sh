#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib/run_hostctl.sh
source "$script_dir/lib/run_hostctl.sh"

reject_legacy_env_vars "test_troubleshoot_hw.sh" \
    ESPFLASH_PORT \
    ESPFLASH_BAUD \
    TROUBLESHOOT_FLASH_FIRST \
    TROUBLESHOOT_FLASH_RETRIES \
    TROUBLESHOOT_PROBE_RETRIES \
    TROUBLESHOOT_PROBE_DELAY_MS \
    TROUBLESHOOT_PROBE_TIMEOUT_MS \
    TROUBLESHOOT_SOAK_CYCLES

build_mode="${1:-debug}"
output_path="${2:-}"

args=(test troubleshoot --build-mode "$build_mode")
if [[ -n "$output_path" ]]; then
    args+=(--output "$output_path")
fi

run_hostctl "${args[@]}"
