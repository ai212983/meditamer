#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib/run_hostctl.sh
source "$script_dir/lib/run_hostctl.sh"

reject_legacy_env_vars "marble_metrics.sh" \
    ESPFLASH_PORT \
    ESPFLASH_BAUD \
    METRICS_SETTLE_MS \
    METRICS_RETRIES \
    METRICS_RETRY_DELAY_MS \
    METRICS_TIMEOUT_MS

run_hostctl marble-metrics
