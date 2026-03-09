#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
app_dir="$repo_root/tools/esp_idf_wifi_control"
build_dir="$repo_root/.embuild/idf_apps/wifi_control/build"

# shellcheck source=../lib/serial_port.sh
source "$script_dir/../lib/serial_port.sh"

resolve_idf_root() {
    if [[ -n "${IDF_APP_ROOT:-}" ]]; then
        printf '%s\n' "$IDF_APP_ROOT"
        return 0
    fi

    local latest=""
    local candidate=""
    shopt -s nullglob
    for candidate in "$repo_root"/.embuild/espressif/esp-idf/v*; do
        [[ -d "$candidate" ]] || continue
        latest="$candidate"
    done
    shopt -u nullglob

    if [[ -n "$latest" ]]; then
        printf '%s\n' "$latest"
        return 0
    fi

    return 1
}

ensure_idf_env() {
    local idf_root
    if ! idf_root="$(resolve_idf_root)"; then
        echo "No local ESP-IDF install found under $repo_root/.embuild/espressif/esp-idf" >&2
        echo "Set IDF_APP_ROOT explicitly if ESP-IDF is installed elsewhere." >&2
        exit 1
    fi
    if [[ ! -f "$idf_root/export.sh" ]]; then
        echo "ESP-IDF export.sh not found at $idf_root/export.sh" >&2
        exit 1
    fi
    # shellcheck disable=SC1090
    source "$idf_root/export.sh" >/dev/null
    if ! command -v idf.py >/dev/null 2>&1; then
        echo "idf.py not available after sourcing $idf_root/export.sh" >&2
        exit 1
    fi
    echo "wifi_control_idf.sh: using ESP-IDF root: $idf_root" >&2
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
