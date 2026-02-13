#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
output_path="${1:-$script_dir/../logs/tap_trace_$(date +%Y%m%d_%H%M%S).log}"

if [[ -z "${ESPFLASH_PORT:-}" ]]; then
    echo "ESPFLASH_PORT must be set (example: /dev/cu.usbserial-540)" >&2
    exit 1
fi

mkdir -p "$(dirname "$output_path")"
output_path="$(cd "$(dirname "$output_path")" && pwd)/$(basename "$output_path")"

echo "Capturing serial output to: $output_path" >&2
echo "Press Ctrl+C to stop." >&2

exec env ESPFLASH_MONITOR_OUTPUT_FILE="$output_path" "$script_dir/monitor.sh"
