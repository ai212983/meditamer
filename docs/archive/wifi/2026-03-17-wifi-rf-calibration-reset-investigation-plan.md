# 2026-03-17 Wi-Fi RF Calibration and Reset Investigation Plan

## Goal

Determine whether the current dark-state behavior is primarily driven by RF /
PHY reset-path differences, persistent non-flash device state,
board-conditioning effects that survive source rollback, or a narrower
firmware-only cause.

The plan is now narrowed further: on the conditioned board, the active app-path
fault domain is the path between successful scan start/completion and
successful AP result population or retrieval.

This plan replaces source-only March 5 vs March 6 reduction as the active line
of investigation until a stable code-only boundary exists again.

## Current Facts

- `meditamer_march_5` can be green on a clean board/workflow.
- `meditamer_march_6` can also be green on a clean board/workflow.
- A reproducible dark trigger now exists on board `08:3a:8d:82:0b:98`:
  - flash `meditamer_march_5`
  - flash pristine `meditamer_march_6`
  - run the one-round discovery gate
- Under that trigger, pristine `meditamer_march_6` is dark:
  - `logs/march6_dark_repro_after_march5_predecessor_round1_20260317.log`
- Under that same trigger, replacing March 6 `prepare_scan.rs` with the March 5
  file is still dark:
  - `logs/march6_after_march5_predecessor_prepare_scan_revert_round1_20260317.log`
- After the board is in that dark condition, flashing `meditamer_march_5` is
  also dark:
  - `logs/march5_after_march6_dark_condition_round1_20260317.log`
- Full flash erase does not restore March 5 on that board:
  - `logs/march5_after_full_erase_round1_20260317.log`

## Interpretation

The strongest currently supported explanation is not:

- PSRAM alone
- a single `prepare_scan.rs` source delta
- normal flash-persisted Wi-Fi state alone

The strongest supported explanation is now a conditioning effect that can push a
board into a dark runtime state that:

- survives March 6 -> March 5 source rollback
- survives full flash erase
- affects both March 5 and March 6 once active

That moves the fault domain toward:

- RF / PHY reset and calibration path
- reset / boot sequencing
- radio or hardware runtime state not reset by ordinary flashing
- or a device-conditioning interaction that is below the `prepare_scan.rs`
  control-flow layer

That interpretation is now constrained by a stronger same-board control:

- the standalone legacy comparator still scans on the conditioned board
- the current app diagnostic build can now reach `scan_rc=0` on that same board
- but the app still completes with `ap_num=0` and an empty scan-done list

So the remaining fault domain is inside the app/runtime-owned Wi-Fi result
population or retrieval path, not a board-global radio blackout.

## Relevant Prior Evidence

- `upload-throughput-history/part-14.md`
  - previous PHY helper attempt could not use `esp_phy_*` symbols as direct
    diagnostics because those symbols were unresolved at final link in this
    stack
- `upload-throughput-history/part-21.md`
  - restoring a legacy-like PHY enable/disable wrapper in current `esp-radio`
    did not restore scan visibility
- external primary-source guidance points to scan-state and RF reset-path
  sensitivity across erase/reset cycles

## Phase 1: Freeze the Dark Trigger

Status: complete

Steps:

1. Use the one-round profile:
   - `logs/wifi-discovery-debug.rounds1.toml`
2. Reproduce a dark run with:
   - `March 5 predecessor -> pristine March 6`
3. Recheck whether `prepare_scan.rs` file swap alone rescues the board.

Result:

- Dark trigger reproduced.
- Full March 5 `prepare_scan.rs` swap did not rescue the conditioned board.

## Phase 2: Check Whether the Condition Generalizes Across Source Versions

Status: complete

Steps:

1. Flash `meditamer_march_5` after the dark March 6 run.
2. Run the same one-round gate.
3. Full erase the board.
4. Flash `meditamer_march_5` again and rerun the same gate.

Result:

- `meditamer_march_5` stayed dark after the board entered the conditioned dark
  state.
- Full erase did not restore March 5.

Conclusion:

- The active dark condition is broader than a March 6 source regression.
- A plain source rollback and a plain flash erase are insufficient to clear it.

## Phase 3: Compare Green vs Dark Reset-Path Behavior

Status: partial

Steps:

1. Capture earliest passive boot logs for the same image on:
   - a currently green board
   - a currently dark-conditioned board
2. Compare for differences in:
   - PHY / RF calibration messages
   - first Wi-Fi init timing
   - scan-start admission failures
   - reset reason / boot sequencing
3. Prefer passive attach and identical image/workflow for both boards.

Primary target image:

- `meditamer_march_5`, because it is simpler and known to be green outside the
  conditioned dark state.

Current result:

- March 5 reset-boot logs were captured on:
  - green board `e8:6b:ea:fb:f1:74`
    - `logs/green_board_march5_reset_boot_20260317.log`
  - dark-trigger board `08:3a:8d:82:0b:98` after the dark March 6 trigger and
    March 5 rollback
    - `logs/dark_board_march5_after_dark_reset_boot_20260317.log`
- The earliest captured boot output does not yet show an obvious RF / PHY
  calibration divergence.
- Both logs show the same broad boot path:
  - normal partition table
  - app load from `0x10000`
  - `BOOT_RESET reason=Some(ChipPowerOn) code=1`
  - PSRAM initialized
- The dark-conditioned board can still fail the March 5 one-round gate even
  when the early reset-boot log looks normal:
  - `logs/dark_board_march5_after_dark_round1_20260317.log`

Interpretation:

- The first visible divergence is later than bootloader / partition / early
  PSRAM init in the currently captured logs.

## Phase 4: Comparator Control on the Dark Board

Status: complete

Steps:

1. Flash the standalone legacy comparator onto the currently dark-conditioned
   board.
2. Check whether scan still works there.

Result:

- On the same board after the dark app-path trigger, the standalone legacy
  comparator still scans successfully:
  - `logs/dark_board_legacy_comparator_after_dark_20260317.log`
- The comparator log shows:
  - `idf_explicit_compare=ok scan_rc=0 ap_num=9`
  - normal queue / ISR activity
  - normal `g_scan` / `g_chm` postcall path

Conclusion:

- The induced dark condition is not a generic radio blackout below all
  firmware.
- It remains specific to the app/runtime path, even though it is broader than a
  single `prepare_scan.rs` source change.

## Phase 5: Same-Board App vs Comparator Scan-Start Boundary

Status: complete

Steps:

1. Keep the conditioned board attached.
2. Flash a diagnostic current-app build with:
   - boot-scan-only mode
   - explicit-first IDF compare
3. Compare that capture against the working legacy comparator capture on the
   same board.

Artifacts:

- current app diagnostic capture:
  - `logs/flash_capture_20260317_150501/capture.log`
- working comparator on same conditioned board:
  - `logs/dark_board_legacy_comparator_after_dark_20260317.log`

Result:

- The current diagnostic app no longer fails at `esp_wifi_scan_start` on this
  board-conditioned state.
- It shows:
  - `idf_explicit_compare_postcall scan_rc=0`
  - stable `g_chm` / `g_scan` postcall state on the comparator-like path
  - `scan_done_count=1`
  - `status=0`
  - `ap_num=0`
- The same-board comparator shows:
  - `idf_explicit_compare=ok scan_rc=0 ap_num=9`
  - stable `g_chm` / `g_scan` postcall state
  - higher queue / ISR activity and a working result harvest path

Conclusion:

- The current best boundary is no longer simple scan-start admission.
- On the same conditioned board, the app/runtime path can enter scan start and
  keep the comparator-like `g_scan` / `g_chm` state, yet still harvest zero APs.
- The remaining divergence is now between:
  - successful scan start / completion
  - and successful AP result population or retrieval in the app/runtime path.

## History-Guided Exclusions

The older investigation records narrow what should not be retried as the next
primary line:

- `docs/development/upload-throughput-history/part-16.md`
- `docs/development/wifi-upload-decision-ledger.md`
- `docs/development/wifi-legacy-old-stack-blob-compatibility-plan.md`

These records, together with today's captures, already reject these as the next
best focus:

- early bootloader / partition / PSRAM bring-up
- plain Rust scan-wrapper argument shaping
- generic queue implementation swaps
- wait-queue wake behavior
- simplified PHY wrapper restoration
- generic RF blackout below all firmware

## Phase 6: Scan Result Population Boundary

Status: active

Goal:

- compare the app path against the working legacy comparator on the same
  conditioned board where scan results should become visible

Artifacts:

- app diagnostic capture:
  - `logs/flash_capture_20260317_150501/capture.log`
- comparator control:
  - `logs/dark_board_legacy_comparator_after_dark_20260317.log`

Current boundary:

- app:
  - `idf_explicit_compare_postcall scan_rc=0`
  - `event scan_done_list status=0 count=0 scan_id=128 scannum=0x0000`
  - `head_ptr=0x0`
- comparator:
  - `idf_explicit_compare=ok scan_rc=0 ap_num=9`
  - `blob_scan ... scannum=0x0009`

Next steps:

1. instrument the current app/vendor path around scan-done list construction
2. compare where `scannum`, head/tail ownership, and AP record visibility first
   diverge from the comparator
3. avoid more early-boot / reset / PSRAM experiments unless a later signal
   points back there

## Stop Conditions

Stop source-level March 5 vs March 6 reduction until at least one of these is
true:

- a stable green/dark source boundary exists again on the same board without
  conditioning drift
- or the reset/calibration comparison shows a narrower mechanism that can be
  targeted directly
