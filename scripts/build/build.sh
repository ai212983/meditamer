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

run_cargo_build() {
    local mode="$1"
    local cmd=(rustup run "$firmware_toolchain" cargo build -Zbuild-std=core,alloc --target "$firmware_target")

    if [[ "$mode" == "release" ]]; then
        cmd+=(--release)
    fi
    if [[ "${CARGO_NO_DEFAULT_FEATURES:-0}" == "1" ]]; then
        cmd+=(--no-default-features)
    fi
    if [[ -n "${CARGO_FEATURES:-}" ]]; then
        cmd+=(--features "$CARGO_FEATURES")
    fi

    (
        cd "$repo_root"
        CARGO_TARGET_DIR="$firmware_target_dir" "${cmd[@]}"
    )
}

case "${1:-}" in
"" | "release")
    run_cargo_build "release"
    ;;
"debug")
    run_cargo_build "debug"
    ;;
*)
    echo "Wrong argument. Only \"debug\"/\"release\" arguments are supported"
    exit 1
    ;;
esac
