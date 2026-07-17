# 2026-03-20 Wi-Fi History Narrowing Follow-up 47

## Objective

Continue the no-hardware green-control strategy from follow-up 46 by testing the
next smallest app-only delta inside the early pre-classification prelude of
`wDev_ProcessRxSucData`.

The specific goal was to remove the app-only dependency on the incoming `a7` value
for the `a12 == 0` case and make the first gate behave more like the comparator.

## Setup

Starting point:

- `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_patch.elf`
- this is the proven-green follow-up 43 control

Selective patch applied:

- function: `wDev_ProcessRxSucData`
- address: `0x400894d7`
- original instruction:
  - `400894d7: 074d       mov.n a4, a7`
- patched instruction:
  - `400894d7: 040c       movi.n a4, 0`

Practical effect:

- before:
  - if `a12 == 0`, the early gate still depends on the incoming `a7`
- after:
  - if `a12 == 0`, the early gate falls through to the shared main-body path
    directly, which is closer to comparator behavior

Patched artifact:

- `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_rxprelude_a4zero_patch.elf`

Verification:

- `400894d7: 040c       movi.n a4, 0`

Flash/capture artifact:

- `logs/flash_capture_20260320_rxprelude_a4zero/capture.log`

## Result

The branch stayed green, but behavior changed.

Observed in `logs/flash_capture_20260320_rxprelude_a4zero/capture.log`:

- first populated `ScanDone` shrank:
  - `event scan_done_list status=0 count=4 scan_id=128 scannum=0x0004 head_ptr=0x3ffc2294 tail_ptr=0x3ffc45bc`
- later scans still recovered to normal green shapes:
  - `event scan_done_list status=0 count=8 scan_id=129 ...`
  - `event scan_done_list status=0 count=9 scan_id=130 ...`
- explicit compare still succeeds at the stable point:
  - `idf_compare=ok scan_rc=0 ap_num=9 records_returned=9`
- pre-retrieval list still materializes normally at the stable point:
  - `scan_list_probe label=idf_compare phase=before_get_ap_num scannum=0x0009 head_ptr=0x3ffc44f4 tail_ptr=0x3ffc4814`

The revived RX-delivery branch also remains live:

- first `ScanDone` patch counters:
  - `wdev_binary_patch_counts ... pp_post_arg25_count=13`
- later `ScanDone` patch counters:
  - `wdev_binary_patch_counts ... pp_post_arg25_count=15`
- `lmac_rx_suc_wrap_diag after=idf_compare count=8`

## Interpretation

This patch is not a root-cause fix, but it is not a no-op either.

What is now proven:

1. The early `wDev_ProcessRxSucData` prelude is a live branch.
2. Its app-only `a12 == 0` handling influences scan-result progression.
3. But that branch is still not the sole decisive cause, because the run remains
   green at the stable explicit-compare checkpoint.

So the state after follow-up 47 is stronger than before:

- follow-up 46 closed the `wdevProcessRxSucDataAll` second-pass gate as primary
- follow-up 47 shows the deeper `wDev_ProcessRxSucData` prelude is materially
  involved
- but not sufficient by itself to explain the whole failure

## Current Narrowed Boundary

The surviving no-hardware target is now the rest of the app-only early prelude in
`wDev_ProcessRxSucData`, especially the special-case branch for nonzero/non-245
`a12` values before the function reaches the shared main classification body.

Most interesting remaining sub-deltas in that window:

- the `65 / 98` special-case path
- the app-only extra gate before the discard path
- the app-only call path around `0x40089501..0x40089509`

## Recommended Next Step

If continuing without JTAG:

1. keep the follow-up 43 green control intact as baseline
2. make one more selective patch inside the remaining app-only `65 / 98`
   special-case prelude
3. avoid any new trampolines or wrapper growth
