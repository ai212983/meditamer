#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mode="${1:-release}"
set_time_after_flash="${FLASH_SET_TIME_AFTER_FLASH:-1}"
timeset_tz="${TIMESET_TZ:-Europe/Berlin}"
resolved_port="${ESPFLASH_PORT:-}"
feature_args=()

if [[ -n "${CARGO_FEATURES:-}" ]]; then
    feature_args+=(--features "$CARGO_FEATURES")
fi

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

case "$mode" in
"release")
    cargo build --release "${feature_args[@]}"
    if [[ -n "$resolved_port" ]]; then
        espflash flash -p "$resolved_port" -c esp32 target/xtensa-esp32-none-elf/release/meditamer
    else
        espflash flash -c esp32 target/xtensa-esp32-none-elf/release/meditamer
    fi
    ;;
"debug")
    cargo build "${feature_args[@]}"
    if [[ -n "$resolved_port" ]]; then
        espflash flash -p "$resolved_port" -c esp32 target/xtensa-esp32-none-elf/debug/meditamer
    else
        espflash flash -c esp32 target/xtensa-esp32-none-elf/debug/meditamer
    fi
    ;;
*)
    echo "Wrong argument. Only \"debug\"/\"release\" arguments are supported"
    exit 1
    ;;
esac

if [[ "$set_time_after_flash" == "1" ]]; then
    if [[ -n "$resolved_port" ]]; then
        echo "Setting device time from timezone: $timeset_tz"
        ESPFLASH_PORT="$resolved_port" TZ="$timeset_tz" "$script_dir/timeset.sh"
    else
        echo "Skipping TIMESET: set ESPFLASH_PORT or connect a single USB serial device." >&2
    fi
fi
