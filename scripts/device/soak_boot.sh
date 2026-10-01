#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/serial_port.sh
source "$script_dir/../lib/serial_port.sh"

cycles="${1:-10}"
window_sec="${SOAK_WINDOW_SEC:-8}"
baud="${ESPFLASH_BAUD:-115200}"
chip="${ESPFLASH_CHIP:-esp32}"
before="${SOAK_MONITOR_BEFORE:-default-reset}"
after="${SOAK_MONITOR_AFTER:-hard-reset}"

ensure_espflash_port "soak_boot.sh" || exit 1
port="${ESPFLASH_PORT}"

if ! [[ "$cycles" =~ ^[0-9]+$ ]] || [[ "$cycles" -lt 1 ]]; then
    echo "cycles must be a positive integer"
    exit 1
fi

if ! [[ "$window_sec" =~ ^[0-9]+$ ]] || [[ "$window_sec" -lt 1 ]]; then
    echo "SOAK_WINDOW_SEC must be a positive integer"
    exit 1
fi

# Accept either legacy startup markers or the current runtime signature.
required_patterns=(
    "core init complete|BOOT_RESET reason="
    "frontlight brightness write: ok|touch: ready phase=boot"
    "display test pattern: ok|sdprobe\\[request\\]: card_detected"
    "render loop: uptime clock|SDDONE id=0 op=probe status=ok code=ok"
)

if [[ "${SOAK_REQUIRE_UPTIME:-0}" == "1" ]]; then
    required_patterns+=("display uptime screen: ok|STATE phase=OPERATING")
fi

run_dir="${SOAK_LOG_DIR:-}"
cleanup_dir=0
if [[ -z "$run_dir" ]]; then
    run_dir="$(mktemp -d -t meditamer_soak.XXXXXX)"
    cleanup_dir=1
else
    mkdir -p "$run_dir"
fi

monitor_pid=""
cleanup() {
    if [[ -n "$monitor_pid" ]]; then
        kill "$monitor_pid" >/dev/null 2>&1 || true
        wait "$monitor_pid" >/dev/null 2>&1 || true
    fi

    if [[ "$cleanup_dir" -eq 1 ]]; then
        rm -rf "$run_dir"
    fi
}
trap cleanup EXIT

passes=0
fails=0

for cycle in $(seq 1 "$cycles"); do
    log_file="$run_dir/cycle_${cycle}.log"
    : >"$log_file"

    espflash monitor \
        -p "$port" \
        -c "$chip" \
        -B "$baud" \
        --before "$before" \
        --after "$after" \
        --non-interactive >"$log_file" 2>&1 &
    monitor_pid="$!"

    sleep "$window_sec"

    kill "$monitor_pid" >/dev/null 2>&1 || true
    wait "$monitor_pid" >/dev/null 2>&1 || true
    monitor_pid=""

    missing_patterns=()
    for pattern in "${required_patterns[@]}"; do
        if ! grep -aEq "$pattern" "$log_file"; then
            missing_patterns+=("$pattern")
        fi
    done

    if [[ "${#missing_patterns[@]}" -eq 0 ]]; then
        passes=$((passes + 1))
        echo "cycle $cycle/$cycles: PASS"
    else
        fails=$((fails + 1))
        echo "cycle $cycle/$cycles: FAIL"
        echo "  log: $log_file"
        for pattern in "${missing_patterns[@]}"; do
            echo "  missing: $pattern"
        done
    fi

    sleep 1

done

echo "soak summary: pass=$passes fail=$fails cycles=$cycles"
if [[ "$fails" -gt 0 ]]; then
    if [[ "$cleanup_dir" -eq 1 ]]; then
        echo "set SOAK_LOG_DIR to keep failed logs"
    fi
    exit 2
fi
