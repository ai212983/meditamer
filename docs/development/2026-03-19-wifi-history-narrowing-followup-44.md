# 2026-03-19 Wi-Fi History Narrowing Follow-up 44

## Objective
Descend one step deeper than the restored `pp_post(25, ...)` handoff and test the immediate consumer seam with minimal additional perturbation.

## Prior Proven Boundary
Follow-up 43 established that a copied-ELF binary patch restoring the direct-local handoff
`lmacProcessRxSucData -> pp_post(25, ...)` recovers green behavior:

- nonzero `ScanDone` counts
- nonzero `scannum` / `head_ptr` before retrieval
- normal clearing after `get_ap_records`

Relevant artifact:
- `logs/flash_capture_20260319_binarypatch_wdev_panic_lmac_pppost/capture.log`

## Static Consumer Mapping
Static disassembly of the current app ELF proves the first consumer of work item `25`:

- `ppTask` dispatches through a jump table for event IDs `0..26`
- table index `25` resolves to the branch that calls `wdevProcessRxSucDataAll`
- in the current build that call is via the existing wrapped symbol in the source build

Relevant symbols in the current app ELF:
- `ppTask = 0x40085108`
- `pp_post = 0x4008e100`
- `lmacProcessRxSucData = 0x4008e268`
- `wdevProcessRxSucDataAll = 0x40089874`
- `wDev_ProcessRxSucData = 0x400894d0`

Key consumer-side callsites in the current app ELF:
- `lmacProcessRxSucData + 0x46`:
  - `4008e2ae: call8 4008e100 <pp_post>`
- `wdevProcessRxSucDataAll + 0x8a`:
  - `400898fe: call8 400894d0 <wDev_ProcessRxSucData>`

## New Diagnostic Step
Added a local direct trampoline in `src/firmware/storage/upload/wifi/connect/wdev_branch_wrap_diag.rs`:

- `wdev_process_rx_suc_data_trampoline(a2, a3, a4, a5)`

The trampoline logs a dedicated ring:
- `wdev_process_rx_binary_wrap_diag`

Two copied-ELF binary patches were then applied:
1. restore the proven green branch by patching the direct-local post:
   - `lmacProcessRxSucData -> pp_post_trampoline`
2. add a deeper direct-local hook:
   - `wdevProcessRxSucDataAll -> wdev_process_rx_suc_data_trampoline`

Patched artifacts:
- `logs/binary_patch_tests/meditamer_pppost_wdevrx_patch.elf`
- `logs/binary_patch_tests/meditamer_pppost_wdevrx_patch_thin.elf`

## First Result: Full Direct Trampoline
Capture:
- `logs/flash_capture_20260319_binarypatch_pppost_wdevrx/capture.log`

Result:
- branch collapsed back to empty-list failure
- no entries reached the new direct ring
- scan ended with:
  - `scan_done_list status=0 count=0`
  - `scannum=0`
  - `head_ptr=0`
- late binary counts still showed the restored post handoff was active:
  - `pp_post_arg25_count=13`

Interpretation:
- adding the deeper direct-local hook perturbed the recovered branch enough that it never reached the `wDev_ProcessRxSucData` call

## Second Result: Thin Direct Trampoline
To reduce perturbation, the same trampoline was simplified so it only records:
- raw args
- return value

It no longer reads any extra diagnostic counters on entry/exit.

Capture:
- `logs/flash_capture_20260319_binarypatch_pppost_wdevrx_thin/capture.log`

Result stayed the same:
- `wdev_process_rx_binary_wrap_diag ... count=0`
- `scan_done_list status=0 count=0`
- `scannum=0`
- `head_ptr=0`
- `pp_post_arg25_count=13`

## What This Proves
Two facts are now solid:

1. The immediate consumer of restored `pp_post(25, ...)` work is `ppTask` case `25`, which leads into `wdevProcessRxSucDataAll`.
2. Even a thin direct-local hook on the next call
   `wdevProcessRxSucDataAll -> wDev_ProcessRxSucData`
   collapses the recovered branch before that call executes.

That makes the current no-JTAG boundary explicit:
- the revived branch is real and reproducible enough to reach `wdevProcessRxSucDataAll`
- but it is too sensitive to tolerate another direct-local trampoline one call deeper

## Current Best Interpretation
The strongest surviving no-hardware result is still the restored green branch from follow-up 43.

The next unresolved seam is inside or immediately after:
- `wdevProcessRxSucDataAll`
- before `wDev_ProcessRxSucData`

But this seam is now sensitive enough that another direct-local hook changes behavior materially.

## Recommended Next Step
Do not keep stacking local trampolines below `pp_post(25, ...)` without hardware debug.

Best next options:
1. Wait for JTAG and inspect the `wdevProcessRxSucDataAll -> wDev_ProcessRxSucData` transition directly.
2. If continuing without hardware, prefer static comparison and extremely selective binary patching over any new general wrapper/trampoline layer.
