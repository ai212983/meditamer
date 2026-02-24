#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ -z "${ESPFLASH_PORT:-}" ]]; then
    echo "ESPFLASH_PORT must be set (example: /dev/cu.usbserial-540)" >&2
    exit 1
fi

echo "Running SD-card burst/backpressure regression on $ESPFLASH_PORT"
echo "This replays a burst UART sequence and asserts completion markers."

exec env SDCARD_TEST_SUITE=burst "$script_dir/test_sdcard_hw.sh" "${1:-debug}" "${2:-}"
