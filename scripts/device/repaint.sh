#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/run_hostctl.sh
source "$script_dir/../lib/run_hostctl.sh"

reject_legacy_env_vars "repaint.sh" \
    ESPFLASH_PORT \
    ESPFLASH_BAUD \
    REPAINT_SETTLE_MS \
    REPAINT_RETRIES \
    REPAINT_RETRY_DELAY_MS \
    REPAINT_WAIT_ACK \
    REPAINT_ACK_TIMEOUT_MS \
    REPAINT_CMD

args=(repaint)
if [[ -n "${1:-}" ]]; then
    args+=(--command "$1")
fi

run_hostctl "${args[@]}"
