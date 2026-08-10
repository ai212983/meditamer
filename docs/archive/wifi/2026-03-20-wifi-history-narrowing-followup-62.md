# Wi-Fi history narrowing follow-up 62 (known-good long window)

Date: 2026-03-20

## What we did
- Flashed the follow-up 43 binary-patched ELF:
  - `logs/binary_patch_tests/meditamer_wdev_panic_lmac_pppost_patch.elf`
- Captured a longer boot window (90s):
  - `logs/flash_capture_20260320_114940/capture.log`

## Observations
- `scan_done_list` remains empty:
  - `scan_done_list status=0 count=0 scan_id=128 ...`
- `idf_explicit_compare_postcall` appears, but still reports empty results:
  - `scan_done_ap_num=0` and `rx_sta=0` / `rx_ap=0`.
- `wdev_binary_patch_counts` still shows activity:
  - `watchdog_count=8 lmac_rx_suc_count=8 pp_post_arg25_count=13`.
- `mac_event_window` (blob-hal) at `idf_explicit_compare_postcall` is:
  - `w1=0x0404801f` (matching the non-green path, not the historical green `0x0604801f`).

## Interpretation
- Extending the capture window does not recover the green behavior.
- The previously green ELF no longer produces AP lists on this board under current conditions, even when explicit compare runs.

## Next steps (proposed)
- Reapply the follow-up 43 patch to the current instrumented ELF so we can compare full `mac_event_window` words 0-11 between:
  - current instrumented unpatched build
  - current instrumented + follow-up 43 patch
