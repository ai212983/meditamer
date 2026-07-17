#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
app_dir="$repo_root/tools/esp_idf_wifi_control"
build_dir="$repo_root/.embuild/idf_apps/wifi_control/build"

# shellcheck source=../lib/esp_idf_env.sh
source "$script_dir/../lib/esp_idf_env.sh"
# shellcheck source=../lib/serial_port.sh
source "$script_dir/../lib/serial_port.sh"

ensure_idf_env() {
    esp_idf_source_env "wifi_control_idf.sh" auto || exit 1
    if ! command -v idf.py >/dev/null 2>&1; then
        echo "idf.py not available after sourcing $ESP_IDF_ROOT_RESOLVED/export.sh" >&2
        exit 1
    fi
}

idf_cmd() {
    idf.py -C "$app_dir" -B "$build_dir" "$@"
}

reset_stale_build_dir() {
    if [[ ! -d "$build_dir" ]]; then
        return 0
    fi
    if [[ -f "$build_dir/CMakeCache.txt" ]]; then
        return 0
    fi
    if [[ "$build_dir" != "$repo_root"/.embuild/idf_apps/wifi_control/build ]]; then
        echo "Refusing to reset unexpected build dir: $build_dir" >&2
        exit 1
    fi
    if find "$build_dir" -mindepth 1 -print -quit | grep -q .; then
        rm -rf "$build_dir"
    fi
}

run_build() {
    ensure_idf_env
    reset_stale_build_dir
    mkdir -p "$build_dir"
    idf_cmd set-target esp32 build
}

run_flash() {
    ensure_espflash_port "wifi_control_idf.sh" || exit 1
    run_build
    idf_cmd -p "$ESPFLASH_PORT" flash
}

run_monitor() {
    ensure_espflash_port "wifi_control_idf.sh" || exit 1
    ensure_idf_env
    idf_cmd -p "$ESPFLASH_PORT" monitor
}

run_fullclean() {
    ensure_idf_env
    reset_stale_build_dir
    mkdir -p "$build_dir"
    idf_cmd fullclean
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
Usage: scripts/device/wifi_control_idf.sh {build|flash|monitor|fullclean}

Notes:
- scan-only mode is the default because CONFIG_WIFI_CONTROL_SSID defaults empty
- to test STA connect, set Wi-Fi config via menuconfig or sdkconfig before build
EOF
    exit 1
    ;;
esac
