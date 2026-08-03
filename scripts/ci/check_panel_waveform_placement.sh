#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
elf="${1:-$repo_root/target/xtensa-esp32-none-elf/release/meditamer}"

if [[ ! -f "$elf" ]]; then
    echo "panel waveform placement: missing release ELF: $elf" >&2
    exit 2
fi

objdump="${XTENSA_OBJDUMP:-}"
if [[ -z "$objdump" ]]; then
    objdump="$(command -v xtensa-esp32-elf-objdump || true)"
fi
if [[ -z "$objdump" ]]; then
    rust_sysroot="$(rustc --print sysroot)"
    objdump="$(find "$rust_sysroot/xtensa-esp-elf" -type f -name xtensa-esp32-elf-objdump -print -quit 2>/dev/null || true)"
fi
if [[ -z "$objdump" || ! -x "$objdump" ]]; then
    echo "panel waveform placement: xtensa-esp32-elf-objdump not found" >&2
    exit 2
fi

symbols="$(mktemp -t meditamer-panel-symbols.XXXXXX)"
trap 'rm -f "$symbols"' EXIT
"$objdump" -t "$elf" >"$symbols"

required_iram=(
    scan_clean_pass
    scan_full_framebuffer_pass
    scan_full_settle_pass
    scan_partial_framebuffer_rows_with_neutral_drain
)
required_dram=(LUT2 LUTB LUTW)

for symbol in "${required_iram[@]}"; do
    if ! grep -Eq "[.]rwtext[[:space:]].*${symbol}" "$symbols"; then
        echo "panel waveform placement: FAIL: $symbol is not in .rwtext" >&2
        exit 1
    fi
done

for symbol in "${required_dram[@]}"; do
    if ! grep -Eq "[.]data[[:space:]].*${symbol}$" "$symbols"; then
        echo "panel waveform placement: FAIL: $symbol is not in .data" >&2
        exit 1
    fi
done

echo "panel waveform placement: PASS"
echo "  iram=${required_iram[*]}"
echo "  dram=${required_dram[*]}"
