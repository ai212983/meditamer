#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/run_hostctl.sh
source "$script_dir/../lib/run_hostctl.sh"

reject_legacy_env_vars "touch_wizard_dump.sh" \
    ESPFLASH_PORT \
    ESPFLASH_BAUD \
    TOUCH_WIZARD_DUMP_TIMEOUT_MS \
    TOUCH_WIZARD_DUMP_RETRIES \
    TOUCH_WIZARD_DUMP_SETTLE_MS

args=(touch-wizard-dump)
if [[ -n "${1:-}" ]]; then
    args+=(--output "$1")
fi

run_hostctl "${args[@]}"
