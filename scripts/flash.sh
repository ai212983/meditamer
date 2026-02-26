#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mode="${1:-release}"
set_time_after_flash="${FLASH_SET_TIME_AFTER_FLASH:-1}"
timeset_tz="${TIMESET_TZ:-Europe/Berlin}"
resolved_port="${ESPFLASH_PORT:-}"
flash_timeout_s="${FLASH_TIMEOUT_SEC:-360}"
flash_status_interval_s="${FLASH_STATUS_INTERVAL_SEC:-15}"
primary_baud="${ESPFLASH_BAUD:-460800}"
fallback_baud="${ESPFLASH_FALLBACK_BAUD:-115200}"
enable_fallback="${ESPFLASH_ENABLE_FALLBACK:-1}"
skip_update_check="${ESPFLASH_SKIP_UPDATE_CHECK:-1}"
feature_args=()
flash_child_pid=""

if [[ -n "${CARGO_FEATURES:-}" ]]; then
    feature_args+=(--features "$CARGO_FEATURES")
fi

validate_positive_integer() {
    local value="$1"
    local name="$2"
    if ! [[ "$value" =~ ^[0-9]+$ ]] || ((value <= 0)); then
        echo "$name must be a positive integer (got: $value)" >&2
        exit 1
    fi
}

validate_toggle() {
    local value="$1"
    local name="$2"
    if [[ "$value" != "0" && "$value" != "1" ]]; then
        echo "$name must be 0 or 1 (got: $value)" >&2
        exit 1
    fi
}

validate_positive_integer "$flash_timeout_s" "FLASH_TIMEOUT_SEC"
validate_positive_integer "$flash_status_interval_s" "FLASH_STATUS_INTERVAL_SEC"
validate_positive_integer "$primary_baud" "ESPFLASH_BAUD"
validate_positive_integer "$fallback_baud" "ESPFLASH_FALLBACK_BAUD"
validate_toggle "$enable_fallback" "ESPFLASH_ENABLE_FALLBACK"
validate_toggle "$skip_update_check" "ESPFLASH_SKIP_UPDATE_CHECK"

cleanup_flash_child() {
    if [[ -n "$flash_child_pid" ]]; then
        kill "$flash_child_pid" >/dev/null 2>&1 || true
        wait "$flash_child_pid" >/dev/null 2>&1 || true
        flash_child_pid=""
    fi
}
trap cleanup_flash_child INT TERM

port_busy_report() {
    if [[ -z "$resolved_port" ]]; then
        return 0
    fi
    if ! lsof "$resolved_port" >/dev/null 2>&1; then
        return 0
    fi
    echo "Serial port is busy: $resolved_port" >&2
    lsof "$resolved_port" >&2 || true
    return 1
}

run_with_timeout() {
    local timeout_s="$1"
    shift
    local start_ts="$SECONDS"
    local next_status_ts=$((start_ts + flash_status_interval_s))

    "$@" &
    flash_child_pid=$!

    while kill -0 "$flash_child_pid" >/dev/null 2>&1; do
        local elapsed=$((SECONDS - start_ts))
        if ((elapsed >= timeout_s)); then
            echo "Flash timed out after ${timeout_s}s. Terminating..." >&2
            kill "$flash_child_pid" >/dev/null 2>&1 || true
            sleep 1
            kill -KILL "$flash_child_pid" >/dev/null 2>&1 || true
            wait "$flash_child_pid" >/dev/null 2>&1 || true
            flash_child_pid=""
            return 124
        fi
        if ((SECONDS >= next_status_ts)); then
            echo "Flashing in progress... elapsed ${elapsed}s"
            next_status_ts=$((SECONDS + flash_status_interval_s))
        fi
        sleep 1
    done

    local status=0
    wait "$flash_child_pid" || status=$?
    flash_child_pid=""
    return "$status"
}

if [[ -z "$resolved_port" ]]; then
    shopt -s nullglob
    candidates=(/dev/cu.usbserial* /dev/tty.usbserial* /dev/cu.usbmodem* /dev/tty.usbmodem*)
    shopt -u nullglob
    if (( ${#candidates[@]} == 1 )); then
        resolved_port="${candidates[0]}"
        echo "Using detected serial port: $resolved_port"
    fi
fi

if [[ -f "$HOME/export-esp.sh" ]]; then
    # Ensure Xtensa toolchain is available for linking.
    # shellcheck disable=SC1090
    source "$HOME/export-esp.sh"
fi

if ! port_busy_report; then
    echo "Close monitor/serial processes using $resolved_port and retry." >&2
    exit 1
fi

image_path=""
case "$mode" in
"release")
    if (( ${#feature_args[@]} > 0 )); then
        cargo build --release "${feature_args[@]}"
    else
        cargo build --release
    fi
    image_path="target/xtensa-esp32-none-elf/release/meditamer"
    ;;
"debug")
    if (( ${#feature_args[@]} > 0 )); then
        cargo build "${feature_args[@]}"
    else
        cargo build
    fi
    image_path="target/xtensa-esp32-none-elf/debug/meditamer"
    ;;
*)
    echo "Wrong argument. Only \"debug\"/\"release\" arguments are supported"
    exit 1
    ;;
esac

espflash_cmd=(espflash flash -c esp32 -B "$primary_baud")
if [[ "$skip_update_check" == "1" ]]; then
    espflash_cmd+=(--skip-update-check)
fi
if [[ -n "$resolved_port" ]]; then
    espflash_cmd+=(-p "$resolved_port")
fi

echo "Flashing image: $image_path"
primary_status=0
run_with_timeout "$flash_timeout_s" "${espflash_cmd[@]}" "$image_path" || primary_status=$?
if ((primary_status != 0)); then
    if [[ "$enable_fallback" != "1" ]]; then
        exit "$primary_status"
    fi
    echo "Primary flash failed (status=$primary_status). Retrying fallback (--no-stub, baud=$fallback_baud)..."
    fallback_cmd=(espflash flash -c esp32 --no-stub -B "$fallback_baud")
    if [[ "$skip_update_check" == "1" ]]; then
        fallback_cmd+=(--skip-update-check)
    fi
    if [[ -n "$resolved_port" ]]; then
        fallback_cmd+=(-p "$resolved_port")
    fi
    run_with_timeout "$((flash_timeout_s * 2))" "${fallback_cmd[@]}" "$image_path"
fi

if [[ "$set_time_after_flash" == "1" ]]; then
    if [[ -n "$resolved_port" ]]; then
        echo "Setting device time from timezone: $timeset_tz"
        ESPFLASH_PORT="$resolved_port" TZ="$timeset_tz" "$script_dir/timeset.sh"
    else
        echo "Skipping TIMESET: set ESPFLASH_PORT or connect a single USB serial device." >&2
    fi
fi
