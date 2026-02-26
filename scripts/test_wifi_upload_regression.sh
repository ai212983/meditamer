#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"

if [[ -z "${ESPFLASH_PORT:-}" ]]; then
    echo "ESPFLASH_PORT must be set (example: /dev/cu.usbserial-510)" >&2
    exit 1
fi

cycles="${WIFI_UPLOAD_CYCLES:-3}"
payload_bytes="${WIFI_UPLOAD_PAYLOAD_BYTES:-524288}"
reset_each_cycle="${WIFI_UPLOAD_RESET_EACH_CYCLE:-0}"
connect_timeout_s="${WIFI_UPLOAD_CONNECT_TIMEOUT_SEC:-45}"
listen_timeout_s="${WIFI_UPLOAD_LISTEN_TIMEOUT_SEC:-75}"
upload_timeout_s="${WIFI_UPLOAD_HTTP_TIMEOUT_SEC:-30}"
stat_timeout_ms="${WIFI_UPLOAD_STAT_TIMEOUT_MS:-30000}"
baud="${ESPFLASH_BAUD:-115200}"
remote_root="${WIFI_UPLOAD_REMOTE_ROOT:-/assets/upload-regression}"
monitor_mode="${WIFI_UPLOAD_MONITOR_MODE:-raw}"
monitor_raw_backend="${WIFI_UPLOAD_MONITOR_RAW_BACKEND:-cat}"
monitor_persist_raw="${WIFI_UPLOAD_MONITOR_PERSIST_RAW:-1}"
monitor_raw_tio_mute="${WIFI_UPLOAD_MONITOR_RAW_TIO_MUTE:-0}"
output_path="${1:-$repo_root/logs/wifi_upload_regression_$(date +%Y%m%d_%H%M%S).log}"
payload_path="${WIFI_UPLOAD_PAYLOAD_PATH:-/tmp/wifi_upload_regression_payload.bin}"

if ! [[ "$cycles" =~ ^[0-9]+$ ]] || ((cycles <= 0)); then
    echo "WIFI_UPLOAD_CYCLES must be a positive integer" >&2
    exit 1
fi
if [[ "$reset_each_cycle" != "0" && "$reset_each_cycle" != "1" ]]; then
    echo "WIFI_UPLOAD_RESET_EACH_CYCLE must be 0 or 1" >&2
    exit 1
fi
if ! [[ "$payload_bytes" =~ ^[0-9]+$ ]] || ((payload_bytes <= 0)); then
    echo "WIFI_UPLOAD_PAYLOAD_BYTES must be a positive integer" >&2
    exit 1
fi
for value_name in connect_timeout_s listen_timeout_s upload_timeout_s stat_timeout_ms; do
    value="${!value_name}"
    if ! [[ "$value" =~ ^[0-9]+$ ]] || ((value <= 0)); then
        echo "${value_name} must be a positive integer" >&2
        exit 1
    fi
done

mkdir -p "$(dirname "$output_path")"
output_path="$(cd "$(dirname "$output_path")" && pwd)/$(basename "$output_path")"

now_ms() {
    python3 -c 'import time; print(int(time.time() * 1000))'
}

port_flag() {
    if stty --help >/dev/null 2>&1; then
        printf -- '-F'
    else
        printf -- '-f'
    fi
}

line_count() {
    wc -l <"$output_path"
}

cleanup() {
    stop_capture
}
trap cleanup EXIT INT TERM

start_capture() {
    mkdir -p "$(dirname "$output_path")"
    touch "$output_path"
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
}

stop_capture() {
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

send_line() {
    printf '%s\r\n' "$1" >&3
}

wait_for_pattern_from_line() {
    local start_line="$1"
    local pattern="$2"
    local timeout_s="$3"
    local deadline=$((SECONDS + timeout_s))
    while ((SECONDS < deadline)); do
        if tail -n +$((start_line + 1)) "$output_path" | rg -q -- "$pattern"; then
            return 0
        fi
        sleep 1
    done
    return 1
}

first_match_from_line() {
    local start_line="$1"
    local pattern="$2"
    tail -n +$((start_line + 1)) "$output_path" | rg -m1 -- "$pattern" || true
}

ack_status_from_line() {
    local start_line="$1"
    local ack_tag="$2"
    local timeout_s="$3"
    local deadline=$((SECONDS + timeout_s))
    while ((SECONDS < deadline)); do
        local match_line
        match_line="$(tail -n +$((start_line + 1)) "$output_path" | rg -m1 -- "${ack_tag} (OK|BUSY|ERR)" || true)"
        if [[ "$match_line" == *" OK"* ]]; then
            echo "OK"
            return 0
        fi
        if [[ "$match_line" == *" BUSY"* ]]; then
            echo "BUSY"
            return 0
        fi
        if [[ "$match_line" == *" ERR"* ]]; then
            echo "ERR"
            return 0
        fi
        sleep 1
    done
    echo "NONE"
    return 0
}

wait_for_sdreq_id_from_line() {
    local start_line="$1"
    local op="$2"
    local timeout_s="$3"
    local deadline=$((SECONDS + timeout_s))
    while ((SECONDS < deadline)); do
        local match_line
        match_line="$(tail -n +$((start_line + 1)) "$output_path" | rg -m1 -- "SDREQ id=[0-9]+ op=${op}" || true)"
        if [[ -n "$match_line" ]]; then
            sed -nE 's/.*SDREQ id=([0-9]+) .*/\1/p' <<<"$match_line"
            return 0
        fi
        sleep 1
    done
    return 1
}

wait_for_sdwait_ok() {
    local request_id="$1"
    local timeout_ms="$2"
    local wait_timeout_s=$((((timeout_ms + 999) / 1000) + 10))
    local start_line
    start_line="$(line_count)"
    send_line "SDWAIT $request_id $timeout_ms"
    if ! wait_for_pattern_from_line "$start_line" "SDWAIT (DONE|TIMEOUT|ERR)" "$wait_timeout_s"; then
        return 1
    fi
    local line
    line="$(first_match_from_line "$start_line" "SDWAIT (DONE|TIMEOUT|ERR)")"
    [[ "$line" == *"SDWAIT DONE"* && "$line" == *"status=ok"* && "$line" == *"code=ok"* ]]
}

set_upload_mode_on() {
    local attempt=1
    while ((attempt <= 20)); do
        local start_line
        start_line="$(line_count)"
        send_line "MODE UPLOAD ON"
        local status
        status="$(ack_status_from_line "$start_line" "MODE" 4)"
        if [[ "$status" == "OK" ]]; then
            return 0
        fi
        if [[ "$status" == "ERR" ]]; then
            echo "MODE UPLOAD ON returned ERR" >&2
            tail -n 120 "$output_path" >&2
            return 1
        fi
        attempt=$((attempt + 1))
        sleep 1
    done
    echo "MODE UPLOAD ON did not return OK" >&2
    tail -n 120 "$output_path" >&2
    return 1
}

set_upload_mode_off() {
    local attempt=1
    while ((attempt <= 12)); do
        local start_line
        start_line="$(line_count)"
        send_line "MODE UPLOAD OFF"
        local status
        status="$(ack_status_from_line "$start_line" "MODE" 4)"
        if [[ "$status" == "OK" ]]; then
            return 0
        fi
        if [[ "$status" == "ERR" ]]; then
            echo "MODE UPLOAD OFF returned ERR" >&2
            tail -n 120 "$output_path" >&2
            return 1
        fi
        attempt=$((attempt + 1))
        sleep 1
    done
    echo "MODE UPLOAD OFF did not return OK" >&2
    tail -n 120 "$output_path" >&2
    return 1
}

maybe_send_wifiset() {
    local start_line="$1"
    local ssid="${WIFI_UPLOAD_SSID:-}"
    if [[ -z "$ssid" ]]; then
        return 0
    fi
    if ! wait_for_pattern_from_line "$start_line" "waiting for WIFISET credentials over UART|SD wifi config invalid; waiting for WIFISET" 4; then
        return 0
    fi
    local password="${WIFI_UPLOAD_PASSWORD:-}"
    local command="WIFISET $ssid"
    if [[ -n "$password" ]]; then
        command="WIFISET $ssid $password"
    fi

    local attempt=1
    while ((attempt <= 8)); do
        local cmd_start
        cmd_start="$(line_count)"
        send_line "$command"
        if wait_for_pattern_from_line "$cmd_start" "WIFISET (OK|BUSY|ERR)" 6; then
            local line
            line="$(first_match_from_line "$cmd_start" "WIFISET (OK|BUSY|ERR)")"
            if [[ "$line" == *"WIFISET OK"* ]]; then
                return 0
            fi
            if [[ "$line" == *"WIFISET ERR"* ]]; then
                echo "WIFISET failed: $line" >&2
                return 1
            fi
        fi
        attempt=$((attempt + 1))
        sleep 1
    done
    echo "WIFISET timed out after retries" >&2
    return 1
}

verify_remote_file_exists() {
    local remote_path="$1"
    local attempt=1
    while ((attempt <= 8)); do
        local start_line
        start_line="$(line_count)"
        send_line "SDFATSTAT $remote_path"
        local ack
        ack="$(ack_status_from_line "$start_line" "SDFATSTAT" 4)"
        if [[ "$ack" == "OK" ]]; then
            local request_id
            request_id="$(wait_for_sdreq_id_from_line "$start_line" "fat_stat" 8 || true)"
            if [[ -n "$request_id" ]] && wait_for_sdwait_ok "$request_id" "$stat_timeout_ms"; then
                return 0
            fi
        fi
        if [[ "$ack" == "ERR" ]]; then
            return 1
        fi
        attempt=$((attempt + 1))
        sleep 1
    done
    return 1
}

calc_kib_per_s() {
    local bytes="$1"
    local ms="$2"
    python3 - "$bytes" "$ms" <<'PY'
import sys
bytes_count = int(sys.argv[1])
elapsed_ms = max(1, int(sys.argv[2]))
kib_per_s = (bytes_count / 1024.0) / (elapsed_ms / 1000.0)
print(f"{kib_per_s:.2f}")
PY
}

calc_mean() {
    printf '%s\n' "$@" | awk '{s+=$1} END { if (NR == 0) { print "0"; } else { printf "%.2f", s/NR; } }'
}

calc_min() {
    printf '%s\n' "$@" | awk 'NR==1{m=$1} $1<m{m=$1} END { if (NR==0) { print "0"; } else { print m; } }'
}

calc_max() {
    printf '%s\n' "$@" | awk 'NR==1{m=$1} $1>m{m=$1} END { if (NR==0) { print "0"; } else { print m; } }'
}

echo "Preparing upload payload: $payload_path (${payload_bytes} bytes)"
python3 - "$payload_path" "$payload_bytes" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
size = int(sys.argv[2])
path.parent.mkdir(parents=True, exist_ok=True)
data = bytearray(size)
for i in range(size):
    data[i] = (i * 31 + 17) & 0xFF
path.write_bytes(data)
PY

echo "Starting monitor capture: $output_path"
start_capture

connect_ms_samples=()
listen_ms_samples=()
upload_ms_samples=()
upload_kib_s_samples=()

total_start_ms="$(now_ms)"

for cycle in $(seq 1 "$cycles"); do
    echo
    echo "=== cycle $cycle/$cycles ==="
    cycle_start_ms="$(now_ms)"
    if [[ "$reset_each_cycle" == "1" ]]; then
        stop_capture
        espflash reset -p "$ESPFLASH_PORT" -c esp32 >/dev/null
        start_capture
    elif ((cycle > 1)); then
        set_upload_mode_off
        sleep 1
    fi

    cycle_start_line="$(line_count)"
    set_upload_mode_on
    maybe_send_wifiset "$cycle_start_line"

    if ! wait_for_pattern_from_line "$cycle_start_line" "upload_http: wifi connected" "$connect_timeout_s"; then
        echo "[FAIL] cycle $cycle: wifi did not connect within ${connect_timeout_s}s" >&2
        tail -n 200 "$output_path" >&2
        exit 1
    fi
    connect_ms="$(( $(now_ms) - cycle_start_ms ))"

    if ! wait_for_pattern_from_line "$cycle_start_line" "upload_http: listening on [0-9]+\\.[0-9]+\\.[0-9]+\\.[0-9]+:8080" "$listen_timeout_s"; then
        echo "[FAIL] cycle $cycle: upload server did not start within ${listen_timeout_s}s" >&2
        tail -n 200 "$output_path" >&2
        exit 1
    fi
    listen_ms="$(( $(now_ms) - cycle_start_ms ))"

    listen_line="$(first_match_from_line "$cycle_start_line" "upload_http: listening on [0-9]+\\.[0-9]+\\.[0-9]+\\.[0-9]+:8080")"
    device_ip="$(sed -nE 's/.*listening on ([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+):8080.*/\1/p' <<<"$listen_line")"
    if [[ -z "$device_ip" ]]; then
        echo "[FAIL] cycle $cycle: failed to parse device IP" >&2
        tail -n 200 "$output_path" >&2
        exit 1
    fi

    cycle_remote_root="${remote_root}/cycle-${cycle}"
    remote_file="${cycle_remote_root}/$(basename "$payload_path")"
    upload_start_ms="$(now_ms)"
    python3 "$script_dir/upload_assets_http.py" \
        --host "$device_ip" \
        --port 8080 \
        --src "$payload_path" \
        --dst "$cycle_remote_root" \
        --timeout "$upload_timeout_s" >/tmp/wifi_upload_cycle_${cycle}.log 2>&1 || {
        echo "[FAIL] cycle $cycle: upload failed" >&2
        cat /tmp/wifi_upload_cycle_${cycle}.log >&2
        tail -n 160 "$output_path" >&2
        exit 1
    }
    upload_ms="$(( $(now_ms) - upload_start_ms ))"

    if ! verify_remote_file_exists "$remote_file"; then
        echo "[FAIL] cycle $cycle: SD verification failed for $remote_file" >&2
        tail -n 200 "$output_path" >&2
        exit 1
    fi

    upload_kib_s="$(calc_kib_per_s "$payload_bytes" "$upload_ms")"
    connect_ms_samples+=("$connect_ms")
    listen_ms_samples+=("$listen_ms")
    upload_ms_samples+=("$upload_ms")
    upload_kib_s_samples+=("$upload_kib_s")

    echo "[PASS] cycle $cycle ip=$device_ip connect_ms=$connect_ms listen_ms=$listen_ms upload_ms=$upload_ms throughput_kib_s=$upload_kib_s"
done

total_ms="$(( $(now_ms) - total_start_ms ))"
connect_avg="$(calc_mean "${connect_ms_samples[@]}")"
listen_avg="$(calc_mean "${listen_ms_samples[@]}")"
upload_avg="$(calc_mean "${upload_ms_samples[@]}")"
throughput_avg="$(calc_mean "${upload_kib_s_samples[@]}")"

echo
echo "Wi-Fi/upload regression summary"
echo "  cycles=$cycles payload_bytes=$payload_bytes total_ms=$total_ms"
echo "  connect_ms avg=$connect_avg min=$(calc_min "${connect_ms_samples[@]}") max=$(calc_max "${connect_ms_samples[@]}")"
echo "  listen_ms  avg=$listen_avg min=$(calc_min "${listen_ms_samples[@]}") max=$(calc_max "${listen_ms_samples[@]}")"
echo "  upload_ms  avg=$upload_avg min=$(calc_min "${upload_ms_samples[@]}") max=$(calc_max "${upload_ms_samples[@]}")"
echo "  throughput_kib_s avg=$throughput_avg min=$(calc_min "${upload_kib_s_samples[@]}") max=$(calc_max "${upload_kib_s_samples[@]}")"
echo "Log: $output_path"
