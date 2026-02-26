#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"

if [[ -z "${ESPFLASH_PORT:-}" ]]; then
    echo "ESPFLASH_PORT must be set (example: /dev/cu.usbserial-540)" >&2
    exit 1
fi

baud="${ESPFLASH_BAUD:-115200}"
mode_settle_ms="${MODE_SMOKE_SETTLE_MS:-0}"
output_path="${1:-$repo_root/logs/runtime_modes_smoke_$(date +%Y%m%d_%H%M%S).log}"
monitor_mode="${MODE_SMOKE_MONITOR_MODE:-raw}"
monitor_raw_backend="${MODE_SMOKE_MONITOR_RAW_BACKEND:-cat}"
monitor_persist_raw="${MODE_SMOKE_MONITOR_PERSIST_RAW:-1}"
monitor_raw_tio_mute="${MODE_SMOKE_MONITOR_RAW_TIO_MUTE:-0}"
post_upload_status_repeats="${MODE_SMOKE_POST_UPLOAD_STATUS_REPEATS:-3}"
post_upload_timeset_repeats="${MODE_SMOKE_POST_UPLOAD_TIMESET_REPEATS:-2}"

if ! [[ "$mode_settle_ms" =~ ^[0-9]+$ ]]; then
    echo "MODE_SMOKE_SETTLE_MS must be a non-negative integer" >&2
    exit 1
fi
if ! [[ "$post_upload_status_repeats" =~ ^[0-9]+$ ]]; then
    echo "MODE_SMOKE_POST_UPLOAD_STATUS_REPEATS must be a non-negative integer" >&2
    exit 1
fi
if ! [[ "$post_upload_timeset_repeats" =~ ^[0-9]+$ ]]; then
    echo "MODE_SMOKE_POST_UPLOAD_TIMESET_REPEATS must be a non-negative integer" >&2
    exit 1
fi

mkdir -p "$(dirname "$output_path")"
output_path="$(cd "$(dirname "$output_path")" && pwd)/$(basename "$output_path")"

port_flag() {
    if stty --help >/dev/null 2>&1; then
        printf -- '-F'
    else
        printf -- '-f'
    fi
}

cleanup() {
    if [[ "${serial_fd_open:-0}" == "1" ]]; then
        exec 3>&-
        serial_fd_open=0
    fi
    if [[ -n "${monitor_pid:-}" ]]; then
        kill "$monitor_pid" >/dev/null 2>&1 || true
        wait "$monitor_pid" >/dev/null 2>&1 || true
        monitor_pid=""
    fi
}
trap cleanup EXIT INT TERM

echo "Starting monitor capture: $output_path"
ESPFLASH_PORT="$ESPFLASH_PORT" \
ESPFLASH_MONITOR_MODE="$monitor_mode" \
ESPFLASH_MONITOR_RAW_BACKEND="$monitor_raw_backend" \
ESPFLASH_MONITOR_PERSIST_RAW="$monitor_persist_raw" \
ESPFLASH_MONITOR_RAW_TIO_MUTE="$monitor_raw_tio_mute" \
ESPFLASH_MONITOR_OUTPUT_FILE="$output_path" \
"$script_dir/monitor.sh" >/dev/null 2>&1 &
monitor_pid=$!
sleep 1

stty "$(port_flag)" "$ESPFLASH_PORT" "$baud" cs8 -cstopb -parenb -ixon -ixoff -crtscts -echo raw clocal
exec 3>"$ESPFLASH_PORT"
serial_fd_open=1

send_line() {
    printf '%s\r\n' "$1" >&3
}

wait_for_pattern_from_line() {
    local start_line="$1"
    local pattern="$2"
    local timeout_s="$3"
    local deadline=$((SECONDS + timeout_s))
    while ((SECONDS < deadline)); do
        if tail -n +$((start_line + 1)) "$output_path" | rg -q "$pattern"; then
            return 0
        fi
        sleep 1
    done
    return 1
}

first_match_from_line() {
    local start_line="$1"
    local pattern="$2"
    tail -n +$((start_line + 1)) "$output_path" | rg -m1 "$pattern" || true
}

psram_samples=()
mode_samples=()
timeset_samples=()

calc_local_tz_offset_minutes() {
    local raw sign hh mm total
    raw="$(date +%z)"
    sign="${raw:0:1}"
    hh="${raw:1:2}"
    mm="${raw:3:2}"
    total=$((10#$hh * 60 + 10#$mm))
    if [[ "$sign" == "-" ]]; then
        total=$((-total))
    fi
    printf '%s' "$total"
}

capture_psram_snapshot() {
    local label="$1"
    local start_line
    start_line="$(wc -l <"$output_path")"
    send_line "PSRAM"
    if ! wait_for_pattern_from_line "$start_line" "PSRAM feature_enabled=" 8; then
        echo "[FAIL] missing PSRAM response for $label" >&2
        tail -n 120 "$output_path" >&2
        exit 1
    fi
    local line
    line="$(first_match_from_line "$start_line" "PSRAM feature_enabled=")"
    psram_samples+=("$label: $line")
    echo "[PASS] psram snapshot: $label"
}

query_mode_status() {
    local expect_upload="${1:-}"
    local expect_assets="${2:-}"
    local start_line
    start_line="$(wc -l <"$output_path")"
    send_line "MODE STATUS"
    if ! wait_for_pattern_from_line "$start_line" "MODE upload=(on|off) assets=(on|off)" 8; then
        echo "[FAIL] missing MODE STATUS response" >&2
        tail -n 120 "$output_path" >&2
        exit 1
    fi
    local line
    line="$(first_match_from_line "$start_line" "MODE upload=(on|off) assets=(on|off)")"
    if [[ -n "$expect_upload" && "$line" != *"upload=$expect_upload"* ]]; then
        echo "[FAIL] MODE STATUS expected upload=$expect_upload, got: $line" >&2
        exit 1
    fi
    if [[ -n "$expect_assets" && "$line" != *"assets=$expect_assets"* ]]; then
        echo "[FAIL] MODE STATUS expected assets=$expect_assets, got: $line" >&2
        exit 1
    fi
    mode_samples+=("$line")
    echo "[PASS] mode status: $line"
}

apply_mode() {
    local name="$1"
    local command="$2"
    local expect_upload="${3:-}"
    local expect_assets="${4:-}"

    local attempt=1
    local ok=0
    while ((attempt <= 8)); do
        local start_line
        start_line="$(wc -l <"$output_path")"
        send_line "$command"
        if wait_for_pattern_from_line "$start_line" "MODE (OK|BUSY|ERR)" 4; then
            local line
            line="$(first_match_from_line "$start_line" "MODE (OK|BUSY|ERR)")"
            if [[ "$line" == *"MODE OK"* ]]; then
                ok=1
                break
            fi
            if [[ "$line" == *"MODE ERR"* ]]; then
                echo "[FAIL] mode command returned error: $line" >&2
                tail -n 120 "$output_path" >&2
                exit 1
            fi
        fi
        attempt=$((attempt + 1))
        sleep 1
    done

    if [[ "$ok" != "1" ]]; then
        echo "[FAIL] mode command failed after retries: $name ($command)" >&2
        tail -n 120 "$output_path" >&2
        exit 1
    fi

    if ((mode_settle_ms > 0)); then
        sleep "$(awk "BEGIN { print $mode_settle_ms / 1000 }")"
    fi

    query_mode_status "$expect_upload" "$expect_assets"
    echo "[PASS] $name"
}

run_timeset_probe() {
    local label="$1"
    local tz_offset_minutes="$2"

    local attempt=1
    local ok=0
    while ((attempt <= 8)); do
        local start_line epoch
        start_line="$(wc -l <"$output_path")"
        epoch="$(date -u +%s)"
        send_line "TIMESET $epoch $tz_offset_minutes"
        if wait_for_pattern_from_line "$start_line" "TIMESET (OK|BUSY)" 4; then
            local line
            line="$(first_match_from_line "$start_line" "TIMESET (OK|BUSY)")"
            if [[ "$line" == *"TIMESET OK"* ]]; then
                timeset_samples+=("$label: $line")
                ok=1
                break
            fi
        fi
        attempt=$((attempt + 1))
        sleep 1
    done

    if [[ "$ok" != "1" ]]; then
        echo "[FAIL] timeset probe failed after retries: $label" >&2
        tail -n 120 "$output_path" >&2
        exit 1
    fi
    echo "[PASS] $label"
}

run_post_upload_uart_regression_checks() {
    if ((post_upload_status_repeats == 0 && post_upload_timeset_repeats == 0)); then
        return
    fi

    echo "Running post-upload UART regression checks..."
    local i tz_offset_minutes
    for ((i = 1; i <= post_upload_status_repeats; i++)); do
        query_mode_status "on" ""
    done

    tz_offset_minutes="$(calc_local_tz_offset_minutes)"
    for ((i = 1; i <= post_upload_timeset_repeats; i++)); do
        run_timeset_probe "timeset probe #$i" "$tz_offset_minutes"
    done
}

echo "Running runtime mode smoke checks..."
query_mode_status
capture_psram_snapshot "baseline"

apply_mode "upload on" "MODE UPLOAD ON" "on" ""
run_post_upload_uart_regression_checks
capture_psram_snapshot "upload_on"

apply_mode "upload off" "MODE UPLOAD OFF" "off" ""
capture_psram_snapshot "upload_off"

apply_mode "assets off" "MODE ASSETS OFF" "" "off"
capture_psram_snapshot "assets_off"

apply_mode "assets on" "MODE ASSETS ON" "" "on"
capture_psram_snapshot "assets_on"

echo
echo "Mode responses:"
for line in "${mode_samples[@]}"; do
    echo "  $line"
done
echo
echo "TIMESET probes:"
for line in "${timeset_samples[@]}"; do
    echo "  $line"
done
echo
echo
echo "PSRAM snapshots:"
for line in "${psram_samples[@]}"; do
    echo "  $line"
done
echo
echo "Runtime mode smoke passed. Log: $output_path"
