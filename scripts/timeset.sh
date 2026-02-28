#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib/run_hostctl.sh
source "$script_dir/lib/run_hostctl.sh"

reject_legacy_env_vars "timeset.sh" \
    ESPFLASH_PORT \
    ESPFLASH_BAUD \
    TIMESET_SETTLE_MS \
    TIMESET_RETRIES \
    TIMESET_RETRY_DELAY_MS \
    TIMESET_WAIT_ACK \
    TIMESET_ACK_TIMEOUT_MS

args=(timeset)
if [[ -n "${1:-}" ]]; then
    args+=(--epoch "$1")
fi
if [[ -n "${2:-}" ]]; then
    args+=(--tz-offset-minutes "$2")
fi

run_hostctl "${args[@]}"
