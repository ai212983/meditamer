#!/usr/bin/env bash
#
# Guards the DRAM recovery in .cargo/config.toml.
#
# We keep jump tables in flash instead of DRAM to give the CPU0 stack ~13 KB
# back. The hazard is IRAM-resident code that dereferences flash rodata: it
# faults if it runs while esp-storage has the flash cache disabled. That shows
# up as a literal-pool word inside .rwtext pointing into the flash-mapped
# rodata window, so we count those and refuse to let the count grow.
#
# The baseline is not zero: esp-hal and the Wi-Fi blob already ship #[ram]
# functions that reference flash. See docs/reference/dram-budget.md.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
elf="${1:-$repo_root/target/xtensa-esp32-none-elf/release/meditamer}"
baseline="${MEDITAMER_IRAM_FLASH_REF_BASELINE:-78}"

if [[ ! -f "$elf" ]]; then
    echo "iram flash refs: missing release ELF: $elf" >&2
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
    echo "iram flash refs: xtensa-esp32-elf-objdump not found" >&2
    exit 2
fi

count="$("$objdump" -s -j .rwtext "$elf" | python3 -c '
import re
import sys

# drom_seg: flash-mapped rodata. Unreadable while the flash cache is disabled.
DROM_START = 0x3F400000
DROM_END = 0x3F800000

hits = 0
for line in sys.stdin:
    match = re.match(r"\s*([0-9a-f]{8})\s+((?:[0-9a-f]{2,8}\s+){1,4})", line)
    if not match:
        continue
    words = "".join(match.group(2).split())
    for i in range(0, len(words) // 8 * 8, 8):
        value = int.from_bytes(bytes.fromhex(words[i : i + 8]), "little")
        if DROM_START <= value < DROM_END:
            hits += 1
print(hits)
')"

if ((count > baseline)); then
    echo "iram flash refs: FAIL: $count literals in .rwtext point into flash rodata (baseline $baseline)" >&2
    echo "  An IRAM function gained a flash-resident constant or jump table." >&2
    echo "  Add its section to ld/rwdata_hook.x, or re-baseline if it is provably cache-safe." >&2
    exit 1
fi

echo "iram flash refs: PASS"
echo "  count=$count baseline=$baseline"
