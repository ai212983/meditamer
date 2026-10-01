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
# functions that reference flash. See docs/references/memory/meditamer-inkplate/budget.md.
#
# Re-baselined 78 -> 87 for the esp-hal 1.1.1 -> 1.2.0 bump. Every one of the
# 87 literals was attributed to the IRAM function that loads it, by matching
# each flash-holding literal address against the `l32r` instructions in a
# disassembly of .rwtext. No first-party symbol appears; the owners are:
#
#   37  esp_hal::gpio::wakeup::{entry_hook, exit_hook, record_wakeup}
#   25  esp_hal::psram::implem::utils::psram_init
#    3  esp_hal::rtc_cntl::sleep::LowPower::sleep
#   22  esp-radio / esp-phy / esp-rtos handlers, esp-hal I2C/SPI IRQ handlers,
#       and the Xtensa vectors (__level_N_interrupt, __user_exception, ...)
#
# The growth is the redesigned wakeup/sleep API this bump moved to: 1.2.0's
# `gpio::wakeup` hooks and `LowPower::sleep` did not exist in 1.1.1 and
# account for 40 of the 87 between them.
#
# One caveat this count cannot settle: the wakeup hooks run at sleep entry and
# exit, which is exactly when the flash cache may be down. Holding a flash
# pointer is not the same as dereferencing one with the cache off, and this
# check only sees the former. The on-device sleep qualification is what
# actually exercises it -- see docs/references/memory/meditamer-inkplate/budget.md.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
elf="${1:-$repo_root/target/xtensa-esp32-none-elf/release/meditamer}"
baseline="${MEDITAMER_IRAM_FLASH_REF_BASELINE:-87}"

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
    echo "  Add its section to config/linker/esp32/rwdata_hook.x, or re-baseline if it is provably cache-safe." >&2
    exit 1
fi

echo "iram flash refs: PASS"
echo "  count=$count baseline=$baseline"
