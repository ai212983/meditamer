# Wi-Fi history narrowing follow-up 61 (known-good ELF on current board)

Date: 2026-03-20

## What we did
- Flashed the previously green binary-patched ELF from follow-up 43:
  - `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_patch.elf`
- Captured boot log: `logs/flash_capture_20260320_114315/capture.log`.
- Compared `mac_event_window` blob-hal snapshots against the historic green capture from follow-up 43:
  - `logs/flash_capture_20260319_binarypatch_wdev_panic_lmac_pppost/capture.log`.

## Observations
- On the current board with the known-good ELF:
  - `scan_done_list count=0` (scan_id=128) and no subsequent nonzero scans.
  - `idf_compare` lines are absent in the boot window.
  - `wdev_binary_patch_counts after=scan_done watchdog_count=8 lmac_rx_suc_count=8 pp_post_arg25_count=13` still appears, even with empty results.
- `mac_event_window` (blob-hal) deltas vs historical green:
  - `after_start_pre_driver_state` differs at `w1`:
    - green: `w1=0x0004801c`
    - current: `w1=0x00008000`
  - green run advances to later stages (`idf_explicit_compare_postcall`, `rust_scan`, `idf_compare`) with `w1=0x0604801f`.
  - current run does not reach those later stages in the boot window.

## Interpretation
- The previously green ELF no longer yields green results on this board under current conditions.
- The first divergence appears at `after_start_pre_driver_state` in the MAC event window, and the flow does not progress to explicit-compare postcall stages.

## Next steps (proposed)
- Extend capture window to confirm whether later stages appear at all.
- If still absent, treat this as a regression in runtime state rather than the patch itself and focus on why `after_start_pre_driver_state` differs.
