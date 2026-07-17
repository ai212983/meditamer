# 2026-03-20 Wi-Fi History Narrowing Follow-up 50

## Objective

Continue the selective no-hardware patch strategy from follow-up 49 by testing
whether the app-only `a12 == 65` special case is materially required for the
green branch.

The specific goal was to remove the `65` admission while keeping the `98`
admission and the rest of the prelude intact.

## Setup

Starting point:

- `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_patch.elf`
- this is still the proven-green follow-up 43 control

Selective patch applied:

- function: `wDev_ProcessRxSucData`
- address: `0x400894e6`
- original instruction:
  - `400894e6: bfcc42     addi a4, a12, -65`
- patched instruction:
  - `400894e6: 9ecc42     addi a4, a12, -98`

Practical effect:

- before:
  - `a12 == 65` and `a12 == 98` both reach the app-only `a9 + 48` gate
- after:
  - only `a12 == 98` can reach that gate
  - `a12 == 65` now falls through the discard branch

Patched artifact:

- `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_rxprelude_drop65_patch.elf`

Verification:

- `400894e6: 9ecc42     addi a4, a12, -98`

Flash/capture artifact:

- `logs/flash_capture_20260320_rxprelude_drop65/capture.log`

## Result

Removing the `65` special case also collapses the green branch back to the stable
empty-list failure.

Observed in `logs/flash_capture_20260320_rxprelude_drop65/capture.log`:

- first and only `ScanDone` is empty:
  - `event scan_done_list status=0 count=0 scan_id=128 scannum=0x0000 head_ptr=0x0 tail_ptr=0x3ffccbd4`
- explicit scan still completes:
  - `idf_explicit_compare_postcall=postcall scan_rc=0 ... scan_done_count=1 scan_done_ap_num=0`
- pre-retrieval list state is empty:
  - `scan_list_snapshot label=event_post_before_get_ap_num scannum=0x0000 head_ptr=0x00000000`

Important surviving signals:

- early RX-delivery activity is still present:
  - `wdev_binary_patch_counts after=scan_done watchdog_count=8 lmac_rx_suc_count=8 pp_post_arg25_count=13`
- so this patch does not kill the revived RX path outright
- it specifically kills progression from revived RX delivery into AP-list
  materialization

## Interpretation

This complements follow-up 49 and materially tightens the boundary.

What is now proven:

1. The app-only `a12 == 65` special case is materially live.
2. The app-only `a12 == 98` special case is also materially live.
3. Removing either one collapses the green branch while preserving early RX
   delivery counters.

That means the decisive no-hardware behavior is no longer just “special-case
entry exists”. It depends on the deeper interaction inside the shared
special-case body after entry.

## Current Narrowed Boundary

The strongest remaining no-hardware target is now the shared body of the
app-only special-case window around:

- `0x400894f1..0x400894fc`

Most likely live interaction:

- byte gate at `a9 + 48`
- later flag word at `a9 + 52`
- final branch into the app-only call path at `0x40089501`

## Recommended Next Step

If continuing without JTAG:

1. keep the follow-up 43 green control intact
2. stop patching the `65 / 98` selectors themselves
3. target the remaining shared body, especially the `a9 + 52` bit-3 test
