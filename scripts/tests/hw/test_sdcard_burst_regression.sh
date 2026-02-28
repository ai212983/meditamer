#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../lib/run_hostctl.sh
source "$script_dir/../../lib/run_hostctl.sh"

reject_legacy_env_vars "test_sdcard_burst_regression.sh" \
    ESPFLASH_PORT \
    ESPFLASH_BAUD \
    SDCARD_TEST_SUITE

build_mode="${1:-debug}"
output_path="${2:-}"

args=(test sdcard-burst-regression --build-mode "$build_mode")
if [[ -n "$output_path" ]]; then
    args+=(--output "$output_path")
fi

run_hostctl "${args[@]}"
