#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib/serial_port.sh
source "$script_dir/lib/serial_port.sh"
output_path="${1:-$script_dir/../logs/touch_trace_$(date +%Y%m%d_%H%M%S).log}"

ensure_espflash_port "touch_capture.sh" || exit 1

mkdir -p "$(dirname "$output_path")"
output_path="$(cd "$(dirname "$output_path")" && pwd)/$(basename "$output_path")"

echo "Capturing serial output to: $output_path" >&2
echo "Touch trace lines are emitted as: touch_trace,ms,count,x0,y0,..." >&2
echo "Decoded touch events are emitted as: touch_event,ms,kind,x,y,..." >&2
echo "Press Ctrl+C to stop." >&2

exec env ESPFLASH_MONITOR_OUTPUT_FILE="$output_path" "$script_dir/monitor.sh"
