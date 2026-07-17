# 2026-03-20 Wi-Fi History Narrowing Follow-up 51

## Objective

Continue the selective no-hardware patch strategy from follow-up 50 by testing
the remaining shared-body flag gate inside the app-only special-case path.

The specific goal was to force the later bit-3 test on the word at `a9 + 52`
true while leaving the earlier selectors and the `a9 + 48` byte gate intact.

## Setup

Starting point:

- `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_patch.elf`
- this is still the proven-green follow-up 43 control

Selective patch applied:

- function: `wDev_ProcessRxSucData`
- address: `0x400894f7`
- original instruction:
  - `400894f7: d948       l32i.n a4, a9, 52`
- patched instruction:
  - `400894f7: 840c       movi.n a4, 8`

Practical effect:

- before:
  - the later `bbsi a4, 3, 0x40089501` depends on the live flag word at
    `a9 + 52`
- after:
  - bit 3 is forced set
  - the special-case path always takes the branch into the app-only call path

Patched artifact:

- `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_rxprelude_forcebit3_patch.elf`

Verification:

- `400894f7: 840c       movi.n a4, 8`

Flash/capture artifact:

- `logs/flash_capture_20260320_rxprelude_forcebit3/capture.log`

## Result

Forcing the bit-3 gate true also collapses the green branch back to the stable
empty-list failure.

Observed in `logs/flash_capture_20260320_rxprelude_forcebit3/capture.log`:

- first and only `ScanDone` is empty:
  - `event scan_done_list status=0 count=0 scan_id=128 scannum=0x0000 head_ptr=0x0 tail_ptr=0x3ffccbd4`
- pre-retrieval list state is empty:
  - `scan_list_snapshot label=event_post_before_get_ap_num scannum=0x0000 head_ptr=0x00000000`
- early RX-delivery counters are still present:
  - `wdev_binary_patch_counts after=scan_done watchdog_count=8 lmac_rx_suc_count=8 pp_post_arg25_count=13`

## Interpretation

This strengthens the same pattern seen in follow-ups 48, 49, and 50.

What is now proven:

1. The shared-body `a9 + 52` flag test is materially live.
2. Blanket forcing that branch into the app-only call path is not harmless.
3. It collapses green while preserving early RX-delivery counters, just like the
   earlier forced/shared-body edits.

That means the remaining decisive behavior is no longer just “does the path reach
this branch”. It depends on the exact live values consumed by the app-only
special-case body.

## Current Narrowed Boundary

The strongest remaining no-hardware target is now the app-only indirect call path
entered at:

- `0x40089501..0x40089509`

What is now closed as standalone explanations:

- selector `a12 == 65`
- selector `a12 == 98`
- byte gate at `a9 + 48`
- bit-3 branch on the word at `a9 + 52`

All of them are live, but blanket forcing any one of them destroys the green
branch while preserving early RX delivery.

## Recommended Next Step

If continuing without JTAG:

1. keep the follow-up 43 green control intact
2. stop forcing the pre-call gates one-by-one
3. treat the remaining strongest no-hardware target as the app-only indirect call
   path at `0x40089501..0x40089509`
