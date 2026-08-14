# 2026-03-20 Wi-Fi History Narrowing Follow-up 52

## Objective

Resolve the remaining ambiguity around the app-side branch target at
`0x40089501..0x40089509` before making any more speculative no-hardware patches.

The earlier linear `objdump` view through this window was misleading because the
branch target is entered in the middle of the surrounding byte stream.

## Setup

Artifacts inspected:

- app green control:
  - `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_patch.elf`
- comparator:
  - `tools/esp_wifi_legacy_nostd_control/target/xtensa-esp32-none-elf/debug/esp_wifi_legacy_nostd_control`

Tool used:

- `xtensa-esp32-elf-gdb` instruction view at the exact branch targets

## Result

The app branch target is not an unknown app-only indirect path.
It resolves directly to the same sniffer callback family as the comparator.

App target disassembly at `0x40089501`:

- `extui a11, a3, 0, 8`
- `mov.n a10, a2`
- `l32r a8, ... (0x4014d5c4 <wDev_SnifferRxData>)`
- `callx8 a8`
- `j 0x40089736`

Comparator analog at `0x40086bc9`:

- `l8ui a4, a7, 48`
- `beqz.n a4, discard`
- `l32i.n a4, a7, 52`
- `bbci a4, 3, discard`
- `mov.n a10, a2`
- `l32i.n a3, a1, 16`
- `extui a11, a3, 0, 8`
- `l32r a8, ... (0x4011b194 <wDev_SnifferRxData>)`
- `callx8 a8`
- `discard`

## Interpretation

This closes the previously suspected “app-only indirect call path” as a primary
code-shape cause.

What is now proven:

1. The app special-case body does call `wDev_SnifferRxData` directly.
2. The comparator special-case body also calls `wDev_SnifferRxData` directly.
3. The surviving split is therefore not the sniffer-call target itself.

That updates the no-hardware boundary again:

- the selector/gate/call structure is live
- but blanket forcing any one gate destroys green
- and the call target itself is shared
- so the remaining decisive factor is now the exact live metadata values feeding
  that shared special-case body, not a different callee

## Current Narrowed Boundary

The strongest remaining no-hardware target is no longer the call target.
It is the live data consumed by the shared special-case body, especially:

- byte at `a9 + 48`
- flag word at `a9 + 52`
- the incoming `a12` classification value
- any coupled descriptor state in the same metadata object

## Recommended Next Step

If continuing without JTAG:

1. stop static patching of the sniffer-call block itself
2. treat the next target as live metadata capture, not code-shape forcing
3. if a new no-hardware run is attempted, make it a minimal logging probe on the
   metadata object used by the green control, not another structural patch
