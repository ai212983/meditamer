#!/usr/bin/env bash
# Build the medinote-waveshare firmware.
#
# Exists for the same reason scripts/build/build.sh and
# boards/waveshare-rlcd42/build.sh do: this target root is outside the shared
# workspace and needs its own toolchain on PATH before cargo can link for
# `xtensa-esp32s3-none-elf`. As of Phase 2 (product and target axis
# completion plan), this crate does compile LVGL from C -- `medinote`'s
# `screen` module and `boards/waveshare-rlcd42`'s flush bridge both need it --
# so it needs the same bindgen sysroot wiring the board's own build.sh does.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

if [[ -f "$HOME/export-esp.sh" ]]; then
    # shellcheck disable=SC1091
    source "$HOME/export-esp.sh"
fi

toolchain=xtensa-esp32s3-elf
if ! command -v "${toolchain}-gcc" >/dev/null 2>&1; then
    echo "need ${toolchain}-gcc; install or update the Espressif toolchain with espup" >&2
    exit 1
fi

sysroot="$("${toolchain}-gcc" -print-sysroot)"
gcc_include="$("${toolchain}-gcc" -print-file-name=include)"
export CROSS_COMPILE="${CROSS_COMPILE:-$toolchain}"
export BINDGEN_EXTRA_CLANG_ARGS_xtensa_esp32s3_none_elf="${BINDGEN_EXTRA_CLANG_ARGS_xtensa_esp32s3_none_elf:-} -isystem $gcc_include -isystem $sysroot/include"

exec rustup run esp cargo build --release "$@"
