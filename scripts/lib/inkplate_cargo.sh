#!/usr/bin/env bash
# Shared build/Clippy driver. Source this file from a command entry point.

inkplate_cargo() {
    local action="$1"
    shift
    local mode="${1:-release}"
    local preset="${2:-default}"
    if [[ "$#" -gt 2 || ( "$mode" != "debug" && "$mode" != "release" ) ||
        ( "$preset" != "default" && "$preset" != "minimal" ) ]]; then
        echo "usage: $0 [debug|release] [default|minimal]" >&2
        return 2
    fi

    local repo_root
    repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
    local target_dir="$repo_root/targets/meditamer-inkplate"
    local artifact_dir="${FIRMWARE_CARGO_TARGET_DIR:-$target_dir/target}"
    local toolchain="${FIRMWARE_RUSTUP_TOOLCHAIN:-esp}"
    local triple="${FIRMWARE_TARGET_TRIPLE:-xtensa-esp32-none-elf}"

    if [[ -f "$HOME/export-esp.sh" ]]; then
        # shellcheck disable=SC1091
        source "$HOME/export-esp.sh"
    fi
    if ! command -v xtensa-esp32-elf-gcc >/dev/null 2>&1; then
        echo "LVGL builds require xtensa-esp32-elf-gcc on PATH" >&2
        return 1
    fi
    local sysroot gcc_include
    sysroot="$(xtensa-esp32-elf-gcc -print-sysroot)"
    gcc_include="$(xtensa-esp32-elf-gcc -print-file-name=include)"
    export CROSS_COMPILE="${CROSS_COMPILE:-xtensa-esp32-elf}"
    export LV_SYSROOT="${LV_SYSROOT:-$sysroot}"
    export BINDGEN_EXTRA_CLANG_ARGS_xtensa_esp32_none_elf="${BINDGEN_EXTRA_CLANG_ARGS_xtensa_esp32_none_elf:-} -isystem $gcc_include -isystem $sysroot/include"
    export RUSTFLAGS="-C link-arg=-nostartfiles -C link-arg=-Tmeditamer-linkall.x -C link-arg=-L$repo_root/config/linker/esp32"

    local cmd=(rustup run "$toolchain" cargo "$action" "-Zbuild-std=core,alloc" --target "$triple")
    [[ "$mode" != "release" ]] || cmd+=(--release)
    if [[ "$preset" == "minimal" || "${CARGO_NO_DEFAULT_FEATURES:-0}" == "1" ]]; then
        cmd+=(--no-default-features)
    fi
    [[ -z "${CARGO_FEATURES:-}" ]] || cmd+=(--features "$CARGO_FEATURES")
    # Production is explicit: enabling a probe feature must not build unrelated
    # binaries that lack the application's allocator or radio hook symbols.
    cmd+=(--bin "${FIRMWARE_BIN:-meditamer}")
    [[ "${CARGO_LOCKED:-1}" == "0" ]] || cmd+=(--locked)
    [[ "$action" != "clippy" ]] || cmd+=(-- -D warnings)
    (
        cd "$target_dir" || exit
        CARGO_TARGET_DIR="$artifact_dir" "${cmd[@]}"
    )
}
