#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mode="${1:-release}"
set_time_after_flash="${FLASH_SET_TIME_AFTER_FLASH:-1}"
timeset_tz="${TIMESET_TZ:-Europe/Berlin}"

if [[ -f "$HOME/export-esp.sh" ]]; then
    # Ensure Xtensa toolchain is available for linking.
    # shellcheck disable=SC1090
    source "$HOME/export-esp.sh"
fi

case "$mode" in
"release")
    cargo build --release
    if [[ -n "${ESPFLASH_PORT:-}" ]]; then
        espflash flash -p "$ESPFLASH_PORT" -c esp32 target/xtensa-esp32-none-elf/release/meditamer
    else
        espflash flash -c esp32 target/xtensa-esp32-none-elf/release/meditamer
    fi
    ;;
"debug")
    cargo build
    if [[ -n "${ESPFLASH_PORT:-}" ]]; then
        espflash flash -p "$ESPFLASH_PORT" -c esp32 target/xtensa-esp32-none-elf/debug/meditamer
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
    if [[ -n "${ESPFLASH_PORT:-}" ]]; then
        echo "Setting device time from timezone: $timeset_tz"
        TZ="$timeset_tz" "$script_dir/timeset.sh"
    else
        echo "Skipping TIMESET: ESPFLASH_PORT is not set (required by scripts/timeset.sh)." >&2
    fi
fi
