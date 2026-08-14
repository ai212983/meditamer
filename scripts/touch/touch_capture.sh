#!/usr/bin/env bash

# Captures raw/decoded touch traces (--mode touch, default) or serial
# tap/event-engine traces (--mode tap) via a passive monitor attach. Merged
# from the former standalone tap_capture.sh by
# docs/archive/host-tooling/scripts-tools-surface-cleanup-ledger.md change set C-403/C-404 --
# the two were identical except for the default output filename and the
# touch-specific trace/event line-format hints below.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/serial_port.sh
source "$script_dir/../lib/serial_port.sh"

usage() {
    echo "usage: $0 [--mode touch|tap] [output_path]" >&2
}

mode="touch"
if [[ "${1:-}" == "--mode" ]]; then
    mode="${2:-}"
    shift 2 || true
fi
case "$mode" in
touch | tap) ;;
*)
    usage
    exit 2
    ;;
esac

output_path="${1:-$script_dir/../../logs/${mode}_trace_$(date +%Y%m%d_%H%M%S).log}"

ensure_espflash_port "touch_capture.sh" || exit 1

mkdir -p "$(dirname "$output_path")"
output_path="$(cd "$(dirname "$output_path")" && pwd)/$(basename "$output_path")"

echo "Capturing serial output to: $output_path" >&2
if [[ "$mode" == "touch" ]]; then
    echo "Touch trace lines are emitted as: touch_trace,ms,count,x0,y0,..." >&2
    echo "Decoded touch events are emitted as: touch_event,ms,kind,x,y,..." >&2
fi
echo "Press Ctrl+C to stop." >&2

exec env ESPFLASH_MONITOR_OUTPUT_FILE="$output_path" "$script_dir/../device/monitor.sh"
