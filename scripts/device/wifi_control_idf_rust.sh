#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
app_dir="$repo_root/tools/esp_idf_wifi_control_rust"
target_triple="xtensa-esp32-espidf"
profile="${RUST_APP_PROFILE:-debug}"

# shellcheck source=../lib/serial_port.sh
source "$script_dir/../lib/serial_port.sh"

resolve_idf_root() {
    if [[ -n "${IDF_APP_ROOT:-}" ]]; then
        printf '%s\n' "$IDF_APP_ROOT"
        return 0
    fi
    return 1
}

ensure_idf_env() {
    local idf_root
    if ! idf_root="$(resolve_idf_root)"; then
        echo "Set IDF_APP_ROOT explicitly for the external ESP-IDF install." >&2
        exit 1
    fi
    if [[ ! -f "$idf_root/export.sh" ]]; then
        echo "ESP-IDF export.sh not found at $idf_root/export.sh" >&2
        exit 1
    fi
    # shellcheck disable=SC1090
    source "$idf_root/export.sh" >/dev/null
    if ! command -v cargo >/dev/null 2>&1; then
        echo "cargo unavailable after sourcing $idf_root/export.sh" >&2
        exit 1
    fi
    echo "wifi_control_idf_rust.sh: using ESP-IDF root: $idf_root" >&2
}

target_dir() {
    printf '%s\n' "$app_dir/target/$target_triple/$profile"
}

image_path() {
    printf '%s\n' "$(target_dir)/esp_idf_wifi_control_rust"
}

run_build() {
    ensure_idf_env
    export ESP_IDF_TOOLS_INSTALL_DIR=fromenv
    export MCU=esp32
    export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }--cfg espidf_time64"
    (
        cd "$app_dir"
        if [[ "$profile" == "release" ]]; then
            cargo +esp build -Zbuild-std=std,panic_abort --target "$target_triple" --release
        else
            cargo +esp build -Zbuild-std=std,panic_abort --target "$target_triple"
        fi
    )
}

run_flash() {
    ensure_espflash_port "wifi_control_idf_rust.sh" || exit 1
    run_build
    espflash flash -c esp32 -B "${ESPFLASH_BAUD:-115200}" --skip-update-check \
        -p "$ESPFLASH_PORT" "$(image_path)"
}

run_monitor() {
    ensure_espflash_port "wifi_control_idf_rust.sh" || exit 1
    ESPFLASH_MONITOR_MODE="${ESPFLASH_MONITOR_MODE:-espflash}" \
        ESPFLASH_PORT="$ESPFLASH_PORT" \
        ESPFLASH_BAUD="${ESPFLASH_BAUD:-115200}" \
        "$script_dir/monitor.sh"
}

run_fullclean() {
    rm -rf "$app_dir/target"
    rm -rf "$app_dir/.embuild"
}

case "${1:-}" in
build)
    run_build
    ;;
flash)
    run_flash
    ;;
monitor)
    run_monitor
    ;;
fullclean)
    run_fullclean
    ;;
*)
    cat >&2 <<'EOF'
Usage: scripts/device/wifi_control_idf_rust.sh {build|flash|monitor|fullclean}

Required env:
- IDF_APP_ROOT=/path/to/external/esp-idf
- IDF_TOOLS_PATH=/path/to/.espressif

Optional env:
- RUST_APP_PROFILE=debug|release (default: debug)
- ESPFLASH_PORT=/dev/cu.usbserial-*
EOF
    exit 1
    ;;
esac
