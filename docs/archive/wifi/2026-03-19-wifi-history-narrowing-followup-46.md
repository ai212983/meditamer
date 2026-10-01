# 2026-03-19 Wi-Fi History Narrowing Follow-up 46

## Objective

Test the strongest remaining no-hardware static candidate from follow-up 45 with a
single selective binary patch, while staying on the proven-green follow-up 43 ELF.

The target was the app-only second-pass gate in `wdevProcessRxSucDataAll` after the
second `hal_mac_rx_get_last_dscr()` call.

## Setup

Starting point:

- `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_patch.elf`
- this is the proven-green no-hardware branch from follow-up 43
- it already restores the direct-local handoff
  `lmacProcessRxSucData -> pp_post(25, ...)`

Selective patch applied:

- function: `wdevProcessRxSucDataAll`
- address: `0x400898a2`
- original instruction:
  - `400898a2: 190c       movi.n a9, 1`
- patched instruction:
  - `400898a2: 170c       movi.n a7, 1`

Practical effect:

- this forces the following existing branch
  - `400898a4: bnez.n a7, 400898c7`
- to always take the continue path
- so the app-specific log/assert path at `0x400898a6..0x400898c4` is bypassed
  unconditionally in the already-green branch

Patched artifact:

- `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_gateforce_patch.elf`

Verification:

- `400898a2: 170c       movi.n a7, 1`
- `400898a4: f7dc       bnez.n a7, 400898c7`

Flash/capture artifact:

- `logs/flash_capture_20260319_binarypatch_pppost_gateforce/capture.log`

## Result

The branch stayed green.

Observed in `logs/flash_capture_20260319_binarypatch_pppost_gateforce/capture.log`:

- first populated `ScanDone`:
  - `event scan_done_list status=0 count=6 scan_id=128 scannum=0x0006 head_ptr=0x3ffc44f4 tail_ptr=0x3ffc4684`
- later populated scans:
  - `event scan_done_list status=0 count=8 scan_id=129 ...`
  - `event scan_done_list status=0 count=9 scan_id=130 ...`
- explicit compare still sees a real list before retrieval:
  - `scan_list_probe label=idf_compare phase=before_get_ap_num scannum=0x0009 head_ptr=0x3ffc44f4 tail_ptr=0x3ffc47b0`
- retrieval still clears it normally:
  - `scan_list_probe label=idf_compare phase=after_get_ap_records scannum=0x0000 head_ptr=0x0`
- explicit compare still succeeds:
  - `idf_compare=ok scan_rc=0 ap_num=9 records_returned=9`

The restored RX-delivery path also remains live:

- `wdev_binary_patch_counts after=scan_done ... pp_post_arg25_count=15`
- `lmac_rx_suc_wrap_diag after=idf_compare count=8`
- repeated forced/comparator-style MAC event words are still visible

## Interpretation

This closes the follow-up 45 static candidate as a primary cause.

Why:

- we took the exact app-only second-pass condition block that looked most
  suspicious in static comparison
- forced it onto the always-continue path
- and the already-green branch remained green with normal list materialization

So the second-pass gate in `wdevProcessRxSucDataAll` is not the decisive split.
It may still differ from the comparator, but it is not carrying the root cause we
were hunting.

## Current Narrowed Boundary

The strongest remaining no-hardware target is now deeper than
`wdevProcessRxSucDataAll`, but still above the too-sensitive direct-local runtime
hook seam.

The next best target is the large app-only pre-classification block at the start of
`wDev_ProcessRxSucData`.

### App-only candidate window

Relevant app window in follow-up 43 ELF:

- `0x400894d2..0x4008950c`

This block includes:

- app-only arithmetic on `a12` / `a10`
- early type/value gating
- early exits to the discard path at `0x40089736`
- an app-only call through a literal function pointer at `0x40089509`

Comparator structure differs materially here:

- its corresponding early block is shorter and reaches the shared main body sooner
- the common RX classification body is much closer to:
  - comparator: around `0x40086bec`
  - app: around `0x40089512`

That makes the surviving no-hardware target:

- the app-only early classification/discard prelude inside `wDev_ProcessRxSucData`
- not the second-pass gate in `wdevProcessRxSucDataAll`

## Recommended Next Step

If continuing without JTAG:

1. do not spend more time on `wdevProcessRxSucDataAll` second-pass gating
2. statically compare the app-only prelude in `wDev_ProcessRxSucData`
3. prefer one selective binary patch there over any new trampoline layer
