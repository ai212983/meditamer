#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib/run_hostctl.sh
source "$script_dir/lib/run_hostctl.sh"

reject_legacy_env_vars "test_sdcard_hw.sh" \
    ESPFLASH_PORT \
    ESPFLASH_BAUD \
    SDCARD_TEST_FLASH_FIRST \
    SDCARD_TEST_VERIFY_LBA \
    SDCARD_TEST_BASE_PATH \
    SDCARD_TEST_SUITE \
    SDCARD_TEST_SDWAIT_TIMEOUT_MS \
    SDCARD_TEST_MONITOR_MODE \
    SDCARD_TEST_MONITOR_RAW_BACKEND \
    SDCARD_TEST_MONITOR_PERSIST_RAW \
    SDCARD_TEST_MONITOR_RAW_TIO_MUTE \
    SDCARD_TEST_MONITOR_PORT

build_mode="${1:-debug}"
output_path="${2:-}"
suite="${HOSTCTL_SDCARD_SUITE:-all}"

args=(test sdcard-hw --build-mode "$build_mode" --suite "$suite")
if [[ -n "$output_path" ]]; then
    args+=(--output "$output_path")
fi

run_hostctl "${args[@]}"
