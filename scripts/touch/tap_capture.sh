#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/serial_port.sh
source "$script_dir/../lib/serial_port.sh"
output_path="${1:-$script_dir/../../logs/tap_trace_$(date +%Y%m%d_%H%M%S).log}"

ensure_espflash_port "tap_capture.sh" || exit 1

mkdir -p "$(dirname "$output_path")"
output_path="$(cd "$(dirname "$output_path")" && pwd)/$(basename "$output_path")"

echo "Capturing serial output to: $output_path" >&2
echo "Press Ctrl+C to stop." >&2

exec env ESPFLASH_MONITOR_OUTPUT_FILE="$output_path" "$script_dir/../device/monitor.sh"
