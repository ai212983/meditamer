#!/usr/bin/env bash
# Build, flash, and capture the Medinote/Waveshare target with exact artifact
# identity. This target is ESP32-S3 and deliberately does not use the
# Inkplate-specific single-production bootloader/partition workflow.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
port="${ESPFLASH_PORT:-/dev/cu.usbmodem21101}"
flash_baud="${ESPFLASH_BAUD:-460800}"
monitor_baud="${MEDINOTE_MONITOR_BAUD:-115200}"
boot_window_sec="${MEDINOTE_BOOT_WINDOW_SEC:-35}"
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
log_dir="${MEDINOTE_FLASH_LOG_DIR:-$repo_root/logs/medinote_flash_$timestamp}"
flash_bin="${MEDINOTE_FLASH_BIN:-medinote-waveshare}"
probe_marker=""
probe_success=""
probe_summary_key=""
case "$flash_bin" in
    medinote-waveshare) build_features=(--features "${MEDINOTE_FEATURES:-crash-screen}") ;;
    wifi-probe)
        build_features=(--no-default-features --features wifi-storage)
        boot_window_sec="${MEDINOTE_BOOT_WINDOW_SEC:-75}"
        probe_marker="WIFI_PROBE state=done "
        probe_success="ok"
        probe_summary_key="wifi_probe"
        ;;
    sd-transport-probe)
        build_features=(--no-default-features --features sd-transport-probe)
        boot_window_sec="${MEDINOTE_BOOT_WINDOW_SEC:-60}"
        probe_marker="SD_PROBE state=done "
        probe_success="ok"
        probe_summary_key="sd_probe"
        ;;
    ble-shared-runtime-probe)
        build_features=(--no-default-features --features shared-ble-runtime)
        probe_marker="BLE_SHARED_RUNTIME_PROBE state=done "
        probe_success="completed"
        probe_summary_key="ble_probe"
        ;;
    panic-screen-probe)
        build_features=(--no-default-features --features crash-screen)
        probe_marker="PANIC_SCREEN_PROBE stage=flush_done "
        probe_success="success"
        probe_summary_key="panic_probe"
        ;;
    *)
        echo "unsupported MEDINOTE_FLASH_BIN: $flash_bin" >&2
        exit 1
        ;;
esac
elf="$script_dir/target/xtensa-esp32s3-none-elf/release/$flash_bin"
partition_table="$script_dir/partitions.csv"

offset_minutes() {
    local zone sign hours minutes total
    zone="$(date +%z)"
    sign="${zone:0:1}"
    hours="${zone:1:2}"
    minutes="${zone:3:2}"
    total=$((10#$hours * 60 + 10#$minutes))
    if [[ "$sign" == "-" ]]; then
        total=$((-total))
    fi
    echo "$total"
}

field_value() {
    local line="$1"
    local field="$2"
    sed -n "s/.*${field}=\([^ ]*\).*/\1/p" <<<"$line"
}

mkdir -p "$log_dir"
"$script_dir/build.sh" --locked --bin "$flash_bin" "${build_features[@]}"

if [[ ! -f "$elf" ]]; then
    echo "built ELF not found: $elf" >&2
    exit 1
fi
if [[ ! -e "$port" ]]; then
    echo "no such port: $port" >&2
    exit 1
fi

cp "$elf" "$log_dir/firmware.elf"
elf_sha256="$(shasum -a 256 "$elf" | awk '{print $1}')"
head="$(git -C "$repo_root" rev-parse HEAD)"
tracked_dirty=false
if [[ -n "$(git -C "$repo_root" status --porcelain --untracked-files=no)" ]]; then
    tracked_dirty=true
fi

{
    echo "target=medinote-waveshare"
    echo "binary=$flash_bin"
    echo "chip=esp32s3"
    echo "port=$port"
    echo "head=$head"
    echo "tracked_dirty=$tracked_dirty"
    echo "elf_sha256=$elf_sha256"
    echo "partition_table=$partition_table"
    echo "flash_baud=$flash_baud"
    echo "monitor_baud=$monitor_baud"
} > "$log_dir/summary.txt"

espflash flash \
    --skip-update-check \
    --chip esp32s3 \
    --port "$port" \
    --baud "$flash_baud" \
    --before usb-reset \
    --partition-table "$partition_table" \
    --target-app-partition factory \
    "$elf" 2>&1 | tee "$log_dir/flash.log"

# Native USB briefly disappears while the S3 resets. Reopen the same explicit
# port once it returns, without toggling DTR, and capture one bounded boot.
for _ in {1..20}; do
    [[ -e "$port" ]] && break
    sleep 0.25
done
if [[ ! -e "$port" ]]; then
    echo "port did not reappear after flash: $port" >&2
    exit 1
fi

stty -f "$port" "$monitor_baud" -hupcl clocal raw -echo
exec 3<> "$port"
: > "$log_dir/capture.log"
: > "$log_dir/time_sync.log"
time_sync_status="waiting"
time_sync_reason="n/a"
time_sync_session=""
time_sync_requested_utc=""
time_sync_requested_offset_min=""
time_sync_utc=""
time_sync_offset_min=""
probe_status="missing"
if [[ -n "$probe_marker" ]]; then
    time_sync_status="not_applicable"
fi
capture_end=$((SECONDS + boot_window_sec))
while ((SECONDS < capture_end)); do
    if IFS= read -r -t 1 line <&3; then
        line="${line%$'\r'}"
        printf '%s\n' "$line" | tee -a "$log_dir/capture.log"

        if [[ -n "$probe_marker" ]]; then
            if [[ "$line" == *"$probe_marker"* ]]; then
                case "$(field_value "$line" result)" in
                    "$probe_success") [[ "$probe_status" == "error" ]] || probe_status="ok" ;;
                    error) probe_status="error" ;;
                    *)
                        if [[ "$flash_bin" != "sd-transport-probe" ]]; then
                            probe_status="error"
                        fi
                        ;;
                esac
            fi
            continue
        fi

        if [[ "$line" == *"TIME_REQUEST "* && -z "$time_sync_session" ]]; then
            time_sync_session="$(field_value "$line" session)"
            nonce="$(field_value "$line" nonce)"
            if [[ -z "$time_sync_session" || -z "$nonce" ]]; then
                time_sync_status="failed"
                time_sync_reason="malformed_time_request"
                printf 'malformed TIME_REQUEST: %s\n' "$line" >> "$log_dir/time_sync.log"
                continue
            fi

            time_sync_requested_utc="$(date -u +%s)"
            time_sync_requested_offset_min="$(offset_minutes)"
            reply="TIME_REPLY version=1 session=${time_sync_session} nonce=${nonce} utc=${time_sync_requested_utc} offset_min=${time_sync_requested_offset_min}"
            printf '%s\n' "$reply" >&3
            printf '%s\n' "$reply" >> "$log_dir/time_sync.log"
            time_sync_status="awaiting_result"
            continue
        fi

        if [[ "$line" == *"TIME_SYNC OK "* && "$time_sync_status" == "awaiting_result" ]]; then
            result_session="$(field_value "$line" session)"
            time_sync_utc="$(field_value "$line" utc)"
            time_sync_offset_min="$(field_value "$line" offset_min)"
            if [[ "$result_session" != "$time_sync_session" || -z "$time_sync_utc" || -z "$time_sync_offset_min" ]]; then
                time_sync_status="failed"
                time_sync_reason="malformed_or_mismatched_time_sync_ok"
            else
                host_now="$(date -u +%s)"
                drift=$((host_now - time_sync_utc))
                if ((drift < 0)); then
                    drift=$((-drift))
                fi
                if ((drift <= 2)) && [[ "$time_sync_offset_min" == "$time_sync_requested_offset_min" ]]; then
                    time_sync_status="ok"
                else
                    time_sync_status="failed"
                    time_sync_reason="verified_time_out_of_tolerance"
                fi
            fi
            printf '%s\n' "$line" >> "$log_dir/time_sync.log"
            continue
        fi

        if [[ "$line" == *"TIME_SYNC ERR "* && "$time_sync_status" == "awaiting_result" ]]; then
            time_sync_status="failed"
            time_sync_reason="device_$(field_value "$line" reason)"
            printf '%s\n' "$line" >> "$log_dir/time_sync.log"
        fi
    fi
done
exec 3>&-

case "$time_sync_status" in
    waiting)
        time_sync_status="failed"
        time_sync_reason="no_time_request"
        ;;
    awaiting_result)
        time_sync_status="failed"
        time_sync_reason="no_time_sync_result"
        ;;
esac

capture_bytes="$(wc -c < "$log_dir/capture.log" | tr -d ' ')"
{
    echo "capture_bytes=$capture_bytes"
    if [[ -n "$probe_marker" ]]; then
        echo "$probe_summary_key=$probe_status"
    fi
    echo "time_sync=$time_sync_status"
    echo "time_sync_requested_utc=${time_sync_requested_utc:-n/a}"
    echo "time_sync_requested_offset_min=${time_sync_requested_offset_min:-n/a}"
    echo "time_sync_utc=${time_sync_utc:-n/a}"
    echo "time_sync_offset_min=${time_sync_offset_min:-n/a}"
    echo "time_sync_reason=$time_sync_reason"
    echo "artifacts=$log_dir"
} >> "$log_dir/summary.txt"

if [[ -n "$probe_marker" ]]; then
    if [[ "$probe_status" != "ok" ]]; then
        echo "medinote flash=ok $probe_summary_key=$probe_status artifacts=$log_dir" >&2
        exit 1
    fi
elif [[ "$time_sync_status" != "ok" ]]; then
    echo "medinote flash=ok time_sync=failed reason=$time_sync_reason artifacts=$log_dir" >&2
    exit 1
fi

echo "medinote flash-capture complete: $log_dir"
