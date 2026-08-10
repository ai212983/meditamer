# 2026-03-19 Wi-Fi History Narrowing Follow-up 43

## Objective

Push one step deeper than the confirmed `lmacProcessRxSucData` binary-patch seam and test the direct local `pp_post(25, ...)` handoff inside `lmacProcessRxSucData`.

## Setup

Starting point from follow-up 42:

- direct local `wDev_ProcessFiq -> wdev_process_panic_watchdog` was patched to a local trampoline and proven live
- direct local `wDev_ProcessFiq -> lmacProcessRxSucData` was patched to a local trampoline and proven live by `ScanDone`
- but the app still ended with an empty scan-result list

New work in this follow-up:

- added a local `pp_post_trampoline(a2, a3)` in `src/firmware/storage/upload/wifi/connect/wdev_branch_wrap_diag.rs`
- kept it in executable RAM near the blob path
- patched three direct local callsites in a copied ELF:
  - `wDev_ProcessFiq + 0x26 -> wdev_process_panic_watchdog_trampoline`
  - `wDev_ProcessFiq + 0x16c -> lmac_process_rx_suc_data_trampoline`
  - `lmacProcessRxSucData + 0x46 -> pp_post_trampoline`
- added one minimal late counter line at `ScanDone`:
  - `watchdog_count`
  - `lmac_rx_suc_count`
  - `pp_post_arg25_count`

Patched artifact:

- `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_patch.elf`

Flash/capture artifacts:

- `logs/flash_capture_20260319_binarypatch_wdev_panic_lmac_pppost/flash.log`
- `logs/flash_capture_20260319_binarypatch_wdev_panic_lmac_pppost/capture.log`

## Static Verification

Patched disassembly confirmed:

- `4008e21e: call8 40080930 <pp_post_trampoline>`
- `4008e41e: call8 40080998 <wdev_process_panic_watchdog_trampoline>`
- `4008e564: call8 400808ec <lmac_process_rx_suc_data_trampoline>`

## Result

This run materially changed the branch.

The app no longer stayed in the empty-list failure family.

Observed in `logs/flash_capture_20260319_binarypatch_wdev_panic_lmac_pppost/capture.log`:

- first nonzero scan:
  - `event scan_done_list status=0 count=8 scan_id=128 scannum=0x0008 head_ptr=0x3ffc2294 tail_ptr=0x3ffc474c`
- later scans also nonzero:
  - `event scan_done_list status=0 count=9 scan_id=129 ...`
  - `event scan_done_list status=0 count=9 scan_id=130 ...`
- explicit compare now sees a real populated list before retrieval:
  - `scan_list_probe label=idf_compare phase=before_get_ap_num scannum=0x0009 head_ptr=0x3ffc4b98 tail_ptr=0x3ffc47b0`
- retrieval clears the list as expected:
  - `scan_list_probe label=idf_compare phase=after_get_ap_records scannum=0x0000 head_ptr=0x0`

Late binary-patch counter line at `ScanDone`:

- `wdev_binary_patch_counts after=scan_done watchdog_count=0 lmac_rx_suc_count=8 pp_post_arg25_count=14`
- later:
  - `watchdog_count=0 lmac_rx_suc_count=8 pp_post_arg25_count=16`

## Interpretation

This is the strongest result so far.

It means:

- the branch can be driven back into a green result-materialization shape without JTAG
- the live boundary is no longer merely "somewhere before RX delivery"
- the strongest current causal seam is now the direct local `lmacProcessRxSucData -> pp_post(25, ...)` handoff

Concretely:

- patching only the direct local watchdog seam did not restore AP results
- patching only the direct local `lmacProcessRxSucData` seam did not restore AP results
- patching the direct local `pp_post(25, ...)` handoff produced real AP lists and stable nonzero `ScanDone`

That does **not** yet prove the root cause is exactly "pp_post is broken" in source terms.
It does prove that the decisive behavioral split is now at or immediately after that direct local handoff.

## Narrowed Boundary

Current strongest boundary:

- `wDev_ProcessFiq` executes
- `lmacProcessRxSucData` executes
- the critical behavioral split is the direct local `pp_post(25, ...)` handoff and its immediate consumer path

The earlier broad hypotheses are now weaker than this one:

- generic scan retrieval failure
- generic ISR setup failure
- generic timer runtime failure
- generic RX callback registration failure

## Best Next Step

Stay on the current binary-patch path and descend one more layer:

1. Identify the immediate consumer of the `pp_post(25, ...)` work item.
2. Compare unpatched app vs patched app at that consumer boundary.
3. Determine whether the original failure is:
   - direct local `pp_post` call-path semantics
   - queue/object preparation for item `25`
   - or the first consumer of item `25`

## Current Board State

The board was flashed with:

- `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_patch.elf`

The captured run ended after returning to the credential wait path.
