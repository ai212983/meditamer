#!/usr/bin/env bash
#
# Guards the BT-stack DRAM reservation in the pinned esp32 linker script.
#
# esp-hal's own memory.x conservatively carves 64 KiB off the bottom of
# dram_seg when the BT controller is linked:
#
#   RESERVE_DRAM = 0x10000 when esp-hal's `__bluetooth` feature is on, else 0
#   dram_seg ORIGIN = 0x3FFAE000 + 8K + RESERVE_DRAM, len = 192K - RESERVE_DRAM
#
# Through esp-hal 1.1.x that value arrived in a `memory_extras.x` written by
# esp-hal's own build script. esp-hal 1.2.0 inlined the computation into its
# memory.x behind a `#IF CARGO_FEATURE("__bluetooth")` directive that only
# esp-hal's build script preprocesses -- and
# `config/linker/esp32/meditamer-memory.x` replaces that file rather than
# extending it, so it never goes through that pipeline.
# `targets/meditamer-inkplate/build.rs`'s `write_memory_extras` therefore
# recomputes RESERVE_DRAM from this crate's own `CARGO_FEATURE_*` variables and
# uses the pinned controller's exact `CONFIG_BTDM_RESERVE_DRAM = 0xdb5c`. That
# value ends at `SOC_MEM_BT_MISC_END = 0x3ffbdb5c`; the remaining 0x24a4 bytes
# in esp-hal's rounded reservation belong to ordinary application DRAM.
#
# That recomputation is a hand-maintained mirror of a transitive feature
# resolution: `esp-radio`'s `ble` feature is the only edge that turns on
# `esp-hal/__bluetooth`, and today only `ble-foundation` and
# `shared-ble-runtime` reach it. Nothing makes that stay true. If a third
# feature ever reaches `esp-radio/ble`, build.rs writes RESERVE_DRAM = 0 while
# esp-hal still believes the BT stack owns low DRAM, dram_seg starts 0xdb5c too
# low, and .data/.bss are laid straight over the BT stack. There
# is no build error and no link error -- just corruption once BLE runs.
#
# So this check does not re-derive the feature graph, which would be the same
# mirror a second time. It asserts the invariant on the built artifact, where
# both halves are directly observable and neither can drift:
#
#   - `.data` is the first section in dram_seg, so its VMA is the region origin
#     rounded up to the section alignment recorded in the ELF.
#   - the BT controller is linked iff `btdm_controller_init` resolves.
#
# Whatever the feature graph does in future, those two must agree.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

# ORIGIN = 0x3FFAE000 + 8K, i.e. RESERVE_DRAM = 0. Keep in step with
# config/linker/esp32/meditamer-memory.x if upstream ever moves the base.
DRAM_SEG_BASE=$((0x3FFB0000))
RESERVE_DRAM_BT=$((0xdb5c))
bt_adapter="$repo_root/vendor/esp-radio-1.0.0-beta.1-bounded/src/ble/os_adapter_esp32.rs"

if [[ ! -f "$bt_adapter" ]]; then
    echo "bt dram reservation: missing pinned ESP32 BT adapter: $bt_adapter" >&2
    exit 2
fi
bt_misc_end_hex="$(sed -n 's/^const SOC_MEM_BT_MISC_END: u32 = \(0x[[:xdigit:]][[:xdigit:]]*\);$/\1/p' "$bt_adapter")"
if [[ -z "$bt_misc_end_hex" ]]; then
    echo "bt dram reservation: cannot read SOC_MEM_BT_MISC_END from $bt_adapter" >&2
    exit 2
fi
bt_controller_span=$((bt_misc_end_hex - DRAM_SEG_BASE))
if ((bt_controller_span != RESERVE_DRAM_BT)); then
    printf 'bt dram reservation: FAIL: configured reservation 0x%x does not match\n' \
        "$RESERVE_DRAM_BT" >&2
    printf '  pinned ESP32 BT adapter end 0x%08x minus DRAM base 0x%08x = 0x%x.\n' \
        "$bt_misc_end_hex" "$DRAM_SEG_BASE" "$bt_controller_span" >&2
    printf '  Update build.rs and this guard from the pinned controller together.\n' >&2
    exit 1
fi
# Overridable for this guard's own self-test only
# (scripts/tests/host/test_check_bt_dram_reservation.sh), which needs to drive
# the failure path without linking a BT image. Never set it in a real run.
BT_MARKER_SYMBOL="${MEDITAMER_BT_MARKER_SYMBOL:-btdm_controller_init}"

if [[ $# -eq 0 ]]; then
    set -- "$repo_root/target/xtensa-esp32-none-elf/release/meditamer"
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
    echo "bt dram reservation: xtensa-esp32-elf-objdump not found" >&2
    exit 2
fi

status=0

for elf in "$@"; do
    name="$(basename "$elf")"

    if [[ ! -f "$elf" ]]; then
        echo "bt dram reservation: missing ELF: $elf" >&2
        exit 2
    fi

    data_section=$("$objdump" -h "$elf" | awk '$2 == ".data" { print $4, $7; exit }')
    if [[ -z "$data_section" ]]; then
        echo "bt dram reservation: $name has no .data section to locate dram_seg" >&2
        exit 2
    fi
    read -r data_vma data_alignment_expr <<< "$data_section"
    if [[ -z "$data_vma" || ! "$data_alignment_expr" =~ ^2\*\*([0-9]+)$ ]]; then
        echo "bt dram reservation: $name has no .data section to locate dram_seg" >&2
        exit 2
    fi
    data_addr=$((16#$data_vma))
    data_alignment=$((1 << BASH_REMATCH[1]))
    unreserved_data_addr=$(((DRAM_SEG_BASE + data_alignment - 1) & -data_alignment))
    reserved_data_addr=$(((DRAM_SEG_BASE + RESERVE_DRAM_BT + data_alignment - 1) & -data_alignment))

    if "$objdump" -t "$elf" | awk -v symbol="$BT_MARKER_SYMBOL" '$NF == symbol { found = 1 } END { exit !found }'; then
        bt_linked=1
    else
        bt_linked=0
    fi

    if ((data_addr == reserved_data_addr)); then
        reserved=1
    elif ((data_addr == unreserved_data_addr)); then
        reserved=0
    else
        printf 'bt dram reservation: FAIL: %s\n' "$name" >&2
        printf '  .data sits at 0x%08x, which is neither aligned dram_seg start 0x%08x\n' \
            "$data_addr" "$unreserved_data_addr" >&2
        printf '  (RESERVE_DRAM = 0) nor 0x%08x (RESERVE_DRAM = 0x%x, alignment = %d).\n' \
            "$reserved_data_addr" "$RESERVE_DRAM_BT" "$data_alignment" >&2
        printf '  The pinned linker script or esp-hal'"'"'s dram_seg base has drifted;\n' >&2
        printf '  re-check config/linker/esp32/meditamer-memory.x against upstream.\n' >&2
        status=1
        continue
    fi

    # Only one direction is a safety property. RESERVE_DRAM is decided once per
    # crate build and applies to every binary linked in it -- upstream esp-hal
    # works the same way -- so a non-BLE binary built alongside a BLE one
    # legitimately carries the reservation. That direction only costs 56,156
    # bytes of DRAM in that binary. The reverse corrupts memory.
    if ((bt_linked && !reserved)); then
        printf 'bt dram reservation: FAIL: %s\n' "$name" >&2
        printf '  The BT controller is linked (%s resolves) but the low 0xdb5c bytes\n' \
            "$BT_MARKER_SYMBOL" >&2
        printf '  were NOT reserved: .data starts at 0x%08x, so .data/.bss are laid\n' \
            "$data_addr" >&2
        printf '  over the ROM BT stack and will be corrupted once BLE runs.\n' >&2
        printf '  Some feature reaching esp-radio/ble is not covered by write_memory_extras\n' >&2
        printf '  in targets/meditamer-inkplate/build.rs -- add its CARGO_FEATURE_* there.\n' >&2
        status=1
        continue
    fi

    note=""
    if ((!bt_linked && reserved)); then
        note="  (reserved without BT: a sibling binary of a BLE-enabled build; safe, costs 56,156 bytes)"
    fi
    printf 'bt dram reservation: PASS: %-26s bt=%d .data=0x%08x%s\n' \
        "$name" "$bt_linked" "$data_addr" "$note"
done

exit "$status"
