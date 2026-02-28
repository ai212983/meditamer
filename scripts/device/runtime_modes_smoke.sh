#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/run_hostctl.sh
source "$script_dir/../lib/run_hostctl.sh"

reject_legacy_env_vars "runtime_modes_smoke.sh" \
    ESPFLASH_PORT \
    ESPFLASH_BAUD \
    MODE_SMOKE_SETTLE_MS \
    MODE_SMOKE_MONITOR_MODE \
    MODE_SMOKE_MONITOR_RAW_BACKEND \
    MODE_SMOKE_MONITOR_PERSIST_RAW \
    MODE_SMOKE_MONITOR_RAW_TIO_MUTE \
    MODE_SMOKE_POST_UPLOAD_STATUS_REPEATS \
    MODE_SMOKE_POST_UPLOAD_TIMESET_REPEATS

args=(test runtime-modes-smoke)
if [[ -n "${1:-}" ]]; then
    args+=("$1")
fi

run_hostctl "${args[@]}"
