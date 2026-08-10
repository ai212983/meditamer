# Upload Throughput History Part 17

## 2026-03-09: exact boot-scan-window promisc visibility is also zero on the failing no-std path

- Reused the existing promiscuous-RX diagnostic in the env-gated boot-scan path by adding a narrow hook before the first scan in:
  - `src/firmware/storage/upload/wifi/connect/promisc_diag.rs`
  - `src/firmware/storage/upload/wifi/connect/boot_scan_diag.rs`
  - `src/firmware/storage/upload/wifi/connect/mod.rs`
- Built the boot-scan app with:
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_PROMISC_DIAG=1`
  - `MEDITAMER_WIFI_EARLY_DRIVER_STATE_DIAG=1`
- Flashed the app-only image to `0x10000` and captured:
  - `logs/boot_scan_only_diag_promiscwindow_20260309_envrun/boot_espflash_monitor.log`

Key evidence from the log:
- The exact boot-scan-window promisc sweep sees zero packets on all sampled channels:
  - channel `8`: `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - channel `1`: `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - channel `6`: `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - channel `11`: `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - aggregate: `total=0 mgmt=0 ctrl=0 data=0 misc=0`
- The same run then immediately fails all three scan variants:
  - direct IDF `NULL`: `scan_id=128`, `ap_num=0`
  - direct IDF explicit broad: `scan_id=129`, `ap_num=0`
  - wrapped Rust broad: `scan_id=130`, `result_count=0`
- This aligns with the already-empty `g_scan` history, empty `g_cnxMgr` pool, and empty raw `ScanDone` list in the same family of runs.

Conclusion:
- in the exact no-std failing window, the radio/driver path is not surfacing any promiscuous RX traffic at all on the sampled busy channels
- that strongly supports the earlier root-cause direction that the failure is before beacon parsing, history update, and BSS admission
- the remaining target is therefore the earliest RX/scan ingress path on the no-std side, not later scan bookkeeping

## 2026-03-09: making `phy_common_clock_enable()` real does not restore RX visibility

- Added a narrow default-off adapter A/B in:
  - `vendor/esp-radio-0.17.0/src/common_adapter.rs`
  - `vendor/esp-radio-0.17.0/src/lib.rs`
  - `src/firmware/storage/upload/wifi/connect/boot_scan_diag.rs`
- The new diagnostic:
  - counts `phy_common_clock_enable()` / `phy_common_clock_disable()` calls
  - exposes the current refcount
  - enables an upstream-style real PHY-clock refcount path only when `MEDITAMER_WIFI_PHY_COMMON_CLOCK_ENABLE_REAL_DIAG=1`
- Rebuilt the boot-scan app with:
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_PROMISC_DIAG=1`
  - `MEDITAMER_WIFI_EARLY_DRIVER_STATE_DIAG=1`
  - `MEDITAMER_WIFI_PHY_COMMON_CLOCK_ENABLE_REAL_DIAG=1`
- Flashed the app-only image to `0x10000` and captured:
  - `logs/boot_scan_only_diag_phyclockreal_20260309_090054/boot_espflash_monitor.log`

Key evidence from the log:
- The PHY-common-clock hook is definitely live in the failing path:
  - after `idf_compare_first`: `enable_calls=91 disable_calls=77 ref_count=14 real_enable=1`
  - after `idf_explicit_compare_first`: `enable_calls=162 disable_calls=136 ref_count=26 real_enable=1`
  - after `rust_scan`: `enable_calls=236 disable_calls=210 ref_count=26 real_enable=1`
- Despite that, the exact pre-scan promisc sweep is still fully dark:
  - channels `8/1/6/11`: all `total=0 mgmt=0 ctrl=0 data=0 misc=0`
- And all three scan variants still fail exactly the same way:
  - direct IDF `NULL`: `scan_id=128`, `ap_num=0`
  - direct IDF explicit broad: `scan_id=129`, `ap_num=0`
  - wrapped Rust broad: `scan_id=130`, `result_count=0`

Conclusion:
- the no-std blob is actively calling the PHY-common-clock hook
- making that hook real, with upstream-style refcounted clock enable/disable, is not sufficient to restore any RX visibility
- this closes the “no-op `phy_common_clock_enable()` is the primary cause” branch
- the next remaining structural targets are above raw clock toggling:
  - no-std runtime bring-up / scheduler task initialization
  - earlier RX ingress enable state before beacon admission

## 2026-03-09: precreating the `esp-rtos` timer task does not change the first-scan blackout

- Added a narrow default-off scheduler A/B in:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/timer_queue.rs`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/mod.rs`
  - `vendor/esp-rtos-0.2.0/src/lib.rs`
  - `src/firmware/storage/upload/mod.rs`
- The new hook:
  - exposes `esp_rtos::precreate_esp_radio_timer_task()`
  - creates the `timer` task eagerly instead of waiting for the first timer arm
  - is enabled only with `MEDITAMER_WIFI_PRECREATE_TIMER_TASK_DIAG=1`
- Rebuilt the boot-scan app with:
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_PROMISC_DIAG=1`
  - `MEDITAMER_WIFI_EARLY_DRIVER_STATE_DIAG=1`
  - `MEDITAMER_WIFI_PRECREATE_TIMER_TASK_DIAG=1`
- Flashed the app-only image to `0x10000` and captured:
  - `logs/boot_scan_only_diag_precreatetimer_20260309_090917/boot_espflash_monitor.log`

Key evidence from the log:
- the eager-precreate hook definitely ran:
  - `wifi_precreate_timer_task_diag result=ok`
- despite that, the exact pre-scan promisc sweep is still completely dark:
  - channels `8/1/6/11`: all `total=0 mgmt=0 ctrl=0 data=0 misc=0`
- and all three scan variants still fail exactly the same way:
  - direct IDF `NULL`: `scan_id=128`, `ap_num=0`
  - direct IDF explicit broad: `scan_id=129`, `ap_num=0`
  - wrapped Rust broad: `scan_id=130`, `result_count=0`
- the end-state telemetry is effectively unchanged:
  - `wifi_mac_isr_count=99`
  - `wifi_rx_cb_count sta=0 ap=0`
  - raw `ScanDone` list still empty

Conclusion:
- lazy creation of the `esp-rtos` timer task is not the primary cause of the first-scan blackout
- this closes the “create the timer task earlier to match upstream `init_tasks()`” branch
- the remaining target is now earlier than timer scheduling and later than high-level driver config:
  - earliest RX ingress enable / receive-path state in the no-std integration

## 2026-03-09: the `g_scan` history table also stays completely untouched across all failing scans

- Added a new scan-history probe in:
  - `src/firmware/storage/upload/wifi/connect/blob_state_diag.rs`
- The probe snapshots the history fields written by `scan_update_scan_history`:
  - `g_scan + 0x110` history count
  - rows at `g_scan + 0x0a4`, `+0x0c4`, `+0x0e4`
  - row metadata bytes `+0x21/+0x22/+0x23`
- Rebuilt the env-gated boot-scan diagnostic app with:
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE=1`
  - `MEDITAMER_WIFI_EARLY_DRIVER_STATE_DIAG=1`
- Flashed the app-only image to `0x10000` via `esptool.py write_flash` and captured:
  - `logs/boot_scan_only_diag_scanhistory_20260309_envrun/boot_espflash_monitor.log`

Key evidence from the log:
- All three failing scan variants still end at zero:
  - direct IDF `NULL`: `scan_id=128`, `ap_num=0`
  - direct IDF explicit broad: `scan_id=129`, `ap_num=0`
  - wrapped Rust broad: `scan_id=130`, `result_count=0`
- The scan-history table remains completely blank at every checkpoint:
  - `history count=0x00` after `idf_compare_first`
  - `history count=0x00` after `idf_explicit_compare_first`
  - `history count=0x00` after `rust_scan`
  - all three sampled rows remain zero-filled and metadata bytes stay `0x00`
- This sits alongside the earlier results that:
  - raw `ScanDone` sees `scannum=0` and `head_ptr=0`
  - the fixed `g_cnxMgr` BSS slots never populate

Conclusion:
- the failing no-std scan path is earlier than `scan_update_scan_history` as well as earlier than fixed-pool BSS admission
- the remaining target is therefore before both of those side effects:
  - either no relevant management frames are reaching the beacon parser at all
  - or the parser is bailing out before history update and before any BSS-pool touch
- the next discriminating step should focus on very early admission / frame-visibility, not later scan bookkeeping

