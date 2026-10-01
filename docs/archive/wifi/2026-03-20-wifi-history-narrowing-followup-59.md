# Wi-Fi history narrowing follow-up 59 (MAC event window words 6-11)

Date: 2026-03-20

## What we did
- Added a non-ISR MAC event window snapshot that logs words 0-11 at each boot-scan stage.
- Captured `logs/flash_capture_20260320_112234/capture.log` with `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`.

## Observations
- `scan_done_list` still reports `count=0` for scan IDs 128/129.
- `idf_compare=ok` reports `ap_num=0` and `records_returned=0`.
- `mac_event_window` snapshots show these transitions:
  - `before_diag_reset`: `w1=0x02008000`, words6_11 include `0x0fff0fff` and tail word `0xa5000c24`.
  - `after_set_mode`: `w1=0x06008000`, words6_11 still `0x0fff0fff` and `0xa5000c24`.
  - `after_start_pre_driver_state`: `w1=0x0004801c`, words6_11 become `0xffff0fff` and `0xa5802d24`.
  - `after_nan_timer_slot_retarget` and later stages (`rust_scan`, `idf_compare`): `w1=0x0404801c` then `0x0404801f`, words6_11 stay `0xffff0fff` and `0xa5802d24`.
- The `hal_mac_get_event_wrap_diag_ext` entries still show the `0x00000800` / `0x00000000` alternating pattern with identical pre/post words per entry.

## Interpretation
- The MAC event window state is changing across setup phases, but none of these transitions correspond to RX callbacks or nonzero scan results.
- Additional registers (words6_11) are stable once the driver is started, suggesting the missing RX is upstream of the scan result queue.

## Next steps (proposed)
- Compare these MAC event window values against the working-state logs (if we can capture a known-good board) to identify which bits diverge earliest.
