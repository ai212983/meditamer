#!/usr/bin/env bash

set -euo pipefail

duration_sec="${1:-7200}"
baud="${ESPFLASH_BAUD:-115200}"
port="${ESPFLASH_PORT:-}"
chip="${ESPFLASH_CHIP:-esp32}"
before="${SOAK_MONITOR_BEFORE:-default-reset}"
after="${SOAK_MONITOR_AFTER:-hard-reset}"
pattern="display uptime screen: ok"

if [[ -z "$port" ]]; then
    echo "ESPFLASH_PORT must be set (example: /dev/cu.usbserial-540)"
    exit 1
fi

if ! [[ "$duration_sec" =~ ^[0-9]+$ ]] || [[ "$duration_sec" -lt 1 ]]; then
    echo "duration_sec must be a positive integer"
    exit 1
fi

log_file="${SOAK_REFRESH_LOG:-$(mktemp -t meditamer_refresh.XXXXXX.log)}"
monitor_pid=""

cleanup() {
    if [[ -n "$monitor_pid" ]]; then
        kill "$monitor_pid" >/dev/null 2>&1 || true
        wait "$monitor_pid" >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT

espflash monitor \
    -p "$port" \
    -c "$chip" \
    -B "$baud" \
    --before "$before" \
    --after "$after" \
    --non-interactive >"$log_file" 2>&1 &
monitor_pid="$!"

sleep "$duration_sec"

kill "$monitor_pid" >/dev/null 2>&1 || true
wait "$monitor_pid" >/dev/null 2>&1 || true
monitor_pid=""

refresh_count="$(grep -Fc "$pattern" "$log_file" || true)"

if grep -Eq "panic|Guru Meditation|core init failed|display uptime screen: failed" "$log_file"; then
    echo "refresh soak: FAIL"
    echo "  duration_sec=$duration_sec"
    echo "  refresh_count=$refresh_count"
    echo "  log=$log_file"
    exit 2
fi

echo "refresh soak: PASS"
echo "  duration_sec=$duration_sec"
echo "  refresh_count=$refresh_count"
echo "  log=$log_file"
