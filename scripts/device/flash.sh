#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/run_hostctl.sh
source "$script_dir/../lib/run_hostctl.sh"

mode="${1:-release}"
case "$mode" in
    debug|release) ;;
    *)
        echo "Wrong argument. Only \"debug\"/\"release\" arguments are supported" >&2
        exit 1
        ;;
esac

if [[ -n "${ESPFLASH_PORT:-}" ]]; then
    export HOSTCTL_PORT="$ESPFLASH_PORT"
fi
if [[ -n "${ESPFLASH_PORT_HINT:-}" ]]; then
    export HOSTCTL_PORT_HINT="$ESPFLASH_PORT_HINT"
fi

if [[ -f "$HOME/export-esp.sh" ]]; then
    # shellcheck disable=SC1090
    source "$HOME/export-esp.sh"
fi

args=(flash-capture --profile "$mode")
if [[ -n "${HOSTCTL_FLASH_CAPTURE_LOG_PATH:-}" ]]; then
    args+=(--log "$HOSTCTL_FLASH_CAPTURE_LOG_PATH")
fi
if [[ -n "${HOSTCTL_FLASH_CAPTURE_MODE:-}" ]]; then
    args+=(--capture-mode "$HOSTCTL_FLASH_CAPTURE_MODE")
fi
if [[ -n "${HOSTCTL_FLASH_CAPTURE_FLASH_MODE:-}" ]]; then
    args+=(--flash-mode "$HOSTCTL_FLASH_CAPTURE_FLASH_MODE")
fi
if [[ -n "${HOSTCTL_FLASH_CAPTURE_IMAGE:-}" ]]; then
    args+=(--image "$HOSTCTL_FLASH_CAPTURE_IMAGE")
fi
if [[ -n "${ESPFLASH_BAUD:-}" ]]; then
    args+=(--flash-baud "$ESPFLASH_BAUD")
fi
if [[ -n "${HOSTCTL_BAUD:-}" ]]; then
    args+=(--baud "$HOSTCTL_BAUD")
fi
if [[ -n "${HOSTCTL_FLASH_CAPTURE_BOOT_WINDOW_MS:-}" ]]; then
    args+=(--boot-window-ms "$HOSTCTL_FLASH_CAPTURE_BOOT_WINDOW_MS")
fi
if [[ -n "${IDF_APP_ROOT:-}" ]]; then
    args+=(--idf-root "$IDF_APP_ROOT")
fi
if [[ -n "${IDF_TOOLS_PATH:-}" ]]; then
    args+=(--idf-tools-path "$IDF_TOOLS_PATH")
fi

run_hostctl "${args[@]}"
