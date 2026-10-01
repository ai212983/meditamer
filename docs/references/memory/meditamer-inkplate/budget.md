# Meditamer / Inkplate memory budget

## Scope

ESP32-WROVER-E, `targets/meditamer-inkplate`, production binary `meditamer`.
BLE diagnostics and Wi-Fi uploads are included in the default composition:
`scripts/build/build.sh release default`. Qualify any replacement BLE composition independently;
linking the controller does not prove active BLE operation.

## Layout and placement

The [linker map](../../../../config/linker/esp32/meditamer-memory.x) and
[reservation generator](../../../../targets/meditamer-inkplate/build.rs) own the layout.

| Region / owner | Contract |
| --- | --- |
| `dram_seg` | `.data`, `.bss` and remaining CPU0 stack; BLE reserves `0xdb5c` bytes at its base, with `.data` aligned above the controller end |
| `dram2_seg` | 113,840 bytes: 45,000-byte active framebuffer followed by 68,736-byte internal heap; 104 bytes remain |
| Classic-only BT memory | BLE compositions release 15,448 bytes once into a second internal heap region; total internal backing is 84,184 bytes |
| PSRAM | Third allocator region; LVGL arena/draw scratch, previous framebuffer, partial-transition storage, FAT state and bulk assets |

[Allocator initialization](../../../../products/meditamer/src/firmware/psram/init.rs)
owns heap registration. The [ROM-stack rules](rom-stack.md) govern `dram2_seg` ordering.
Keep DMA storage, CPU stacks, interrupt/radio state, scan context and waveform
LUTs internal. The active framebuffer remains internal; LVGL's L8 draw buffer
[already uses PSRAM](../../../../products/meditamer/src/firmware/ui/lvgl/backend/init.rs).
Runtime asset bundles use fallible strict PSRAM allocation; their request ceiling
is not reserved capacity or guaranteed admission.

Jump tables use flash, with interrupt dispatch tables and LLVM constant pools
retained internally by [the hook](../../../../config/linker/esp32/rwdata_hook.x).
Removing that hook can introduce cache-disabled flash accesses.

## Acceptance

- [BLE ELF guard](../../../../scripts/ci/check_ble_image_budget.sh): linked stack
  at least 33,900 bytes; board-runtime pool at most 72 bytes.
- [Stack guard](../../../../scripts/ci/check_stack_risk.sh): bounded external-allocation
  construction frames. Ordinary serial dispatch has a fallible 2,048-byte ceiling.
- [Radio admission](../../../../platform/connectivity/netstack/src/owner.rs): stable
  current internal free of at least 20,496 bytes and a 4,112-byte allocatable block
  while preserving the 16,384-byte reserve.
- Reservation, pinned-linker, panel-placement and IRAM-reference guards are listed in
  [memory validation](../../../guides/memory/validation.md). Script constants own thresholds.

The [Phase 1S runtime gate](../../../../tools/hostctl/src/workflows/ble_phase1s/mod.rs)
also requires CPU0 runtime stack headroom of at least 8,192 bytes (12,288 target),
touch-core headroom of at least 1,024 bytes, BLE-active internal free of at least
16,384 bytes, and post-warm-up free/largest-block drift of at most 1,024 bytes.
These runtime checks complement the linked-layout guards above.

Wi-Fi acceptance uses current free memory. The boot-lifetime minimum remains
diagnostic evidence of transient pressure, not an admission-time reading.
A recovered free snapshot does not erase that pressure; neither value alone
establishes a complete runtime safety bound.

## Qualification

Earlier exclusive handoff passed 20 cycles on commit
`9606e152e816215449486f286cb400bc52d08bab`. Wi-Fi discovery, one/three-cycle
acceptance and six-cycle soak passed on ELF SHA-256
`fde8c6f5ec7622a86872e44f40fc61765ca7b8e417eb33f4784d1768911ae9c3`
(2026-08-14). The latter compared live free memory, retaining historical minima
for diagnosis. These results predate the current reservation/allocator changes.
The exact BT reservation and Classic-region reclaim remain unqualified on the
current candidate; follow [placement and runtime qualification](../../../guides/memory/validation.md#placement-and-runtime-qualification).
