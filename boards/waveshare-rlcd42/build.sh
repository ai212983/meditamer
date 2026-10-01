#!/usr/bin/env bash
# Build the Waveshare S3 board library against its real target.
#
# Since the product and target axis completion plan's Phase 2, this crate is
# a `[lib]` -- board hardware only, no flashed firmware -- so this checks that
# it still compiles for `xtensa-esp32s3-none-elf`; the shipped binary is
# `targets/medinote-waveshare/build.sh`'s job now.
#
# Exists for the same reason scripts/build/build.sh does: LVGL is compiled from
# C by lightvgl-sys's build script, and bindgen needs the Xtensa sysroot on its
# include path or every stdint type comes back "unknown type name". Those paths
# are discovered from the toolchain, so they cannot live in .cargo/config.toml's
# static [env] table.
#
# Note the target-suffixed variable name: bindgen looks for
# BINDGEN_EXTRA_CLANG_ARGS_<target with dashes as underscores>, so the esp32s3
# spelling here is not interchangeable with the root script's esp32 one.

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
