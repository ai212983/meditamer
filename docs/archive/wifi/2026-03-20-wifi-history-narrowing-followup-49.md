# 2026-03-20 Wi-Fi History Narrowing Follow-up 49

## Objective

Continue the selective no-hardware patch strategy from follow-up 48 by testing
whether the app-only `a12 == 98` special case is materially required for the
green branch.

The specific goal was to remove the `98` admission while keeping the `65`
admission and the rest of the prelude intact.

## Setup

Starting point:

- `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_patch.elf`
- this is still the proven-green follow-up 43 control

Selective patch applied:

- function: `wDev_ProcessRxSucData`
- address: `0x400894eb`
- original instruction:
  - `400894eb: 9ecc42     addi a4, a12, -98`
- patched instruction:
  - `400894eb: bfcc42     addi a4, a12, -65`

Practical effect:

- before:
  - `a12 == 65` and `a12 == 98` both reach the app-only `a9 + 48` gate
- after:
  - only `a12 == 65` can reach that gate
  - `a12 == 98` now falls through the discard branch

Patched artifact:

- `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_rxprelude_drop98_patch.elf`

Verification:

- `400894eb: bfcc42     addi a4, a12, -65`

Flash/capture artifact:

- `logs/flash_capture_20260320_rxprelude_drop98/capture.log`

## Result

Removing the `98` special case collapses the green branch back to the stable
empty-list failure.

Observed in `logs/flash_capture_20260320_rxprelude_drop98/capture.log`:

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

This is a strong discriminator.

What is now proven:

1. The app-only `a12 == 98` special case is materially live.
2. `65` and `98` are not interchangeable at this seam.
3. The `98` path is required for the green branch to reach AP-list
   materialization in the current no-hardware control.

Together with follow-up 48, the no-hardware boundary is now much tighter:

- forcing the `a9 + 48` gate nonzero destroys green
- removing the `98` special case also destroys green
- both changes preserve early RX-delivery counters
- so the remaining decisive behavior is concentrated inside the app-only
  special-case subpath itself, not earlier RX restoration

## Current Narrowed Boundary

The strongest remaining no-hardware target is now the app-only special-case
window around:

- `0x400894e9..0x400894f4`

Most likely live interaction:

- `a12 == 98` classification
- `a9 + 48` gate value
- later flag test at `a9 + 52`

## Recommended Next Step

If continuing without JTAG:

1. keep the follow-up 43 green control intact
2. target the remaining `a12 == 65` path separately, not the `98` path again
3. avoid blanket forcing of `a9 + 48`, since follow-up 48 already closed that
