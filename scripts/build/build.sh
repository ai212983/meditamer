#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
firmware_target_dir="${FIRMWARE_CARGO_TARGET_DIR:-$repo_root/target}"

firmware_toolchain="${FIRMWARE_RUSTUP_TOOLCHAIN:-esp}"
firmware_target="${FIRMWARE_TARGET_TRIPLE:-xtensa-esp32-none-elf}"

if [[ -f "$HOME/export-esp.sh" ]]; then
    # Ensure Xtensa toolchain is available for linking.
    # shellcheck disable=SC1090
    source "$HOME/export-esp.sh"
fi

configure_lvgl_toolchain() {
    if ! command -v xtensa-esp32-elf-gcc >/dev/null 2>&1; then
        echo "LVGL builds require xtensa-esp32-elf-gcc on PATH" >&2
        exit 1
    fi

    local lv_sysroot
    local lv_gcc_include
    lv_sysroot="$(xtensa-esp32-elf-gcc -print-sysroot)"
    lv_gcc_include="$(xtensa-esp32-elf-gcc -print-file-name=include)"
    export CROSS_COMPILE="${CROSS_COMPILE:-xtensa-esp32-elf}"
    export LV_SYSROOT="${LV_SYSROOT:-$lv_sysroot}"
    export BINDGEN_EXTRA_CLANG_ARGS_xtensa_esp32_none_elf="${BINDGEN_EXTRA_CLANG_ARGS_xtensa_esp32_none_elf:-} -isystem $lv_gcc_include -isystem $lv_sysroot/include"
}

configure_lvgl_toolchain

apply_profile_args() {
    local profile="$1"
    local profile_features=""
    PROFILE_ARGS=()

    case "$profile" in
    "default")
        ;;
    "minimal")
        PROFILE_ARGS+=(--no-default-features)
        ;;
    "slim")
        PROFILE_ARGS+=(--no-default-features)
        profile_features="wifi-debug-slim-app"
        ;;
    "telemetry")
        profile_features="telemetry-defmt"
        ;;
    "all-features")
        PROFILE_ARGS+=(--all-features)
        ;;
    *)
        echo "Unknown firmware profile: $profile" >&2
        echo "Supported profiles: default, minimal, slim, telemetry, all-features" >&2
        exit 2
        ;;
    esac

    if [[ "${CARGO_NO_DEFAULT_FEATURES:-0}" == "1" && "$profile" != "minimal" && "$profile" != "slim" ]]; then
        PROFILE_ARGS+=(--no-default-features)
    fi

    local requested_features="${CARGO_FEATURES:-}"
    if [[ -n "$profile_features" && -n "$requested_features" ]]; then
        requested_features="$profile_features,$requested_features"
    elif [[ -n "$profile_features" ]]; then
        requested_features="$profile_features"
    fi
    if [[ -n "$requested_features" ]]; then
        PROFILE_ARGS+=(--features "$requested_features")
    fi
}

run_cargo_build() {
    local mode="$1"
    local profile="$2"
    local cmd=(rustup run "$firmware_toolchain" cargo build -Zbuild-std=core,alloc --target "$firmware_target")

    if [[ "$mode" == "release" ]]; then
        cmd+=(--release)
    elif [[ "$mode" == "ble-release" ]]; then
        cmd+=(--profile ble-release)
    fi
    apply_profile_args "$profile"
    cmd+=("${PROFILE_ARGS[@]}")
    if [[ "${CARGO_LOCKED:-1}" != "0" ]]; then
        cmd+=(--locked)
    fi

    (
        cd "$repo_root"
        CARGO_TARGET_DIR="$firmware_target_dir" "${cmd[@]}"
    )
}

run_cargo_clippy() {
    local profile="$1"
    local cmd=(rustup run "$firmware_toolchain" cargo clippy -Zbuild-std=core,alloc --target "$firmware_target" --workspace --bins --lib)
    apply_profile_args "$profile"
    cmd+=("${PROFILE_ARGS[@]}")
    if [[ "${CARGO_LOCKED:-1}" != "0" ]]; then
        cmd+=(--locked)
    fi
    cmd+=(-- -D warnings)

    (
        cd "$repo_root"
        CARGO_TARGET_DIR="$firmware_target_dir" "${cmd[@]}"
    )
}

mode="${1:-release}"
if [[ -n "${2:-}" ]]; then
    profile="$2"
elif [[ "$mode" == "clippy" ]]; then
    # Preserve the historical `build.sh clippy` all-features behavior.
    profile="all-features"
else
    profile="default"
fi

if [[ "$#" -gt 2 ]]; then
    echo "usage: $0 [debug|release|ble-release|clippy] [default|minimal|slim|telemetry|all-features]" >&2
    exit 2
fi

case "$mode" in
"" | "release")
    run_cargo_build "release" "$profile"
    ;;
"ble-release")
    run_cargo_build "ble-release" "$profile"
    ;;
"debug")
    run_cargo_build "debug" "$profile"
    ;;
"clippy")
    run_cargo_clippy "$profile"
    ;;
*)
    echo "usage: $0 [debug|release|ble-release|clippy] [default|minimal|slim|telemetry|all-features]" >&2
    exit 2
    ;;
esac
