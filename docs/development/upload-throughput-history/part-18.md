# Upload Throughput History Part 18

## 2026-03-09: the fixed `g_cnxMgr` BSS pool stays completely empty across all failing scans

- Added a narrow fixed-pool probe in:
  - `src/firmware/storage/upload/wifi/connect/blob_state_diag.rs`
- The new snapshot logs the four `g_cnxMgr` slots walked by `cnx_bss_alloc`:
  - slot base `g_cnxMgr + 0x08`
  - stride `0x3b8`
  - sampled fields: BSSID bytes, `word_0c`, `word_2a8`, `word_2ac`
- Rebuilt the boot-scan diagnostic app with:
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE=1`
  - `MEDITAMER_WIFI_EARLY_DRIVER_STATE_DIAG=1`
- Flashed the app-only image to the factory partition at `0x10000` via `esptool.py write_flash` and captured:
  - `logs/boot_scan_only_diag_cnxslots_20260309_envrun/boot_espflash_monitor.log`

Key evidence from the log:
- All three failing scan variants still end at zero:
  - direct IDF `NULL`: `scan_id=128`, `ap_num=0`
  - direct IDF explicit broad: `scan_id=129`, `ap_num=0`
  - wrapped Rust broad: `scan_id=130`, `result_count=0`
- The four fixed `g_cnxMgr` slots remain entirely blank at every checkpoint:
  - after `idf_compare_first`: all four slots show `bssid=00:00:00:00:00:00`, `word_0c=0`, `word_2a8=0`, `word_2ac=0`
  - after `idf_explicit_compare_first`: same
  - after `rust_scan`: same
- Raw `ScanDone` remains empty at the same checkpoints:
  - `scannum=0x0000`
  - `head_ptr=0x0`
  - `tail_ptr=0x3ffd3318`

Conclusion:
- the failing no-std scan path does not just end with an empty exported result list
- it also never populates the fixed `g_cnxMgr` BSS pool that `cnx_bss_alloc` / `cnx_update_bss` would use
- together with the allocator probe, this means the observed Wi-Fi allocation activity is most likely adjacent scan machinery, not successful BSS admission
- the next root-cause target is therefore earlier in admission:
  - `check_bss_queue`
  - early `scan_parse_beacon` filter/reject branches before `cnx_bss_alloc`

## 2026-03-09: standalone legacy no-std `esp-wifi 0.15.1` scans successfully on the same board and network

- Added a standalone comparator tool outside the main firmware integration:
  - `tools/esp_wifi_legacy_nostd_control/Cargo.toml`
  - `tools/esp_wifi_legacy_nostd_control/.cargo/config.toml`
  - `tools/esp_wifi_legacy_nostd_control/src/main.rs`
- The tool uses published crates rather than the vendored main-firmware stack:
  - `esp-wifi = 0.15.1`
  - `esp-hal = 1.0.0-rc.0`
  - builtin scheduler
  - direct `start -> scan_n(16) -> stop`
- Build notes that mattered:
  - explicit `esp-sync = 0.1.1` with `esp32` feature to fix chip-feature propagation
  - `esp-hal` needed `rt`
  - direct `esp-alloc` had to match the legacy stack (`0.8.0`) to avoid duplicate global allocators
  - the correct linker is the installed legacy `xtensa-esp32-elf-gcc`, not the newer forced `xtensa-esp-elf-gcc`
- Built the tool and flashed app-only to `0x10000` via `esptool.py write_flash`.
- Captured bounded monitor log:
  - `logs/esp_wifi_legacy_nostd_control_20260309_095109/monitor.log`

Key evidence from the log:
- Boot and init are clean:
  - `legacy_nostd_wifi_control: init=ok`
  - `legacy_nostd_wifi_control: wifi_new=ok`
  - `legacy_nostd_wifi_control: set_mode=sta`
  - `legacy_nostd_wifi_control: start=ok`
- The legacy standalone no-std stack actually discovers APs:
  - `legacy_nostd_wifi_control: scan=ok count=3`
  - APs logged include:
    - `<test-ssid-guest>`
    - `<test-ssid-primary>`
- The same tool then stops cleanly:
  - `legacy_nostd_wifi_control: stop=ok`

Conclusion:
- this is the strongest discriminator so far
- the board, environment, and no-std execution model can scan successfully
- the blackout is therefore not a generic “all no-std ESP32 Wi-Fi fails here” problem
- the primary regression boundary moves up to the current `esp-radio` / `esp-rtos` stack used by the main firmware and the standalone `esp_radio_nostd_wifi_control` probe
- the next step should compare current `esp-radio` / `esp-rtos` startup and RX-ingress behavior directly against this working legacy no-std `esp-wifi` path

## 2026-03-09: explicit scheduler-yield handoff in the standalone current `esp-radio` probe does not restore scan visibility

- Updated the standalone current-stack comparator to use the vendored runtime under test:
  - `tools/esp_radio_nostd_wifi_control/Cargo.toml`
  - path-deps to `vendor/esp-radio-0.17.0` and `vendor/esp-rtos-0.2.0`
- Updated the standalone tool entrypoint:
  - `tools/esp_radio_nostd_wifi_control/src/main.rs`
- Added a small `diag_yield()` helper that calls `esp_rtos::yield_for_esp_radio_diag()`:
  - `8x` after `esp_rtos::start()`
  - `8x` after `esp_radio::init()`
  - `8x` after `wifi_new`
  - `16x` after `wifi_start`
- Rebuilt the tool, flashed app-only to `0x10000`, and captured:
  - `logs/esp_radio_nostd_wifi_control_yielddiag_20260309_095543/monitor.log`

Key evidence from the log:
- The explicit handoff points all ran:
  - `diag_yield label=after_rtos_start count=8`
  - `diag_yield label=after_esp_radio_init count=8`
  - `diag_yield label=after_wifi_new count=8`
  - `diag_yield label=after_wifi_start count=16`
- The current standalone stack still fails in the same way:
  - `nostd_wifi_control: start=ok`
  - `nostd_wifi_control: scan=ok count=0`
  - `nostd_wifi_control: stop=ok`

Conclusion:
- this closes the narrow “current stack just needs an explicit scheduler handoff/yield during init/start” branch
- the regression boundary remains in the current `esp-radio` / `esp-rtos` runtime contract, but not in a missing one-time yield alone
- the next step should focus on deeper task/timer/queue semantics in `esp-rtos` relative to the legacy built-in scheduler that still scans successfully

## 2026-03-09: holding the Wi-Fi radio awake across the boot-scan window does not restore RX visibility

- Added a narrow boot-scan-only RF-hold A/B in:
  - `src/firmware/storage/upload/wifi/connect/boot_scan_diag.rs`
- New diagnostic gate:
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_FORCE_WAKEUP_DIAG=1`
- The boot-scan repro now:
  - calls `esp_wifi_force_wakeup_acquire()` immediately after `start=ok`
  - keeps that reference across the pre-scan promisc sweep, direct IDF `NULL` scan, direct IDF explicit broad scan, and wrapped Rust broad scan
  - then calls `esp_wifi_force_wakeup_release()` before `stop_async()`
- Rebuilt the env-gated boot-scan app, flashed app-only to `0x10000` via `esptool.py write_flash`, and captured:
  - `logs/boot_scan_only_diag_forcewakeup_20260309_092120/boot_espflash_monitor.log`

Key evidence from the log:
- The RF-hold API succeeded cleanly:
  - `force_wakeup_acquire rc=0`
  - `force_wakeup_release rc=0`
- Despite that, the exact pre-scan promisc window remained fully dark:
  - `boot_scan_only_promisc_diag outcome=ok ... total=0 mgmt=0 ctrl=0 data=0 misc=0`
- Raw `ScanDone` list state remained empty for all three scan variants:
  - `scan_id=128 ... scannum=0x0000 head_ptr=0x0`
  - `scan_id=129 ... scannum=0x0000 head_ptr=0x0`
  - `scan_id=130 ... scannum=0x0000 head_ptr=0x0`
- All three scan entry paths still failed:
  - direct IDF `NULL`: `idf_compare=ok ... ap_num=0 records_returned=0`
  - direct IDF explicit broad: `idf_explicit_compare=ok ... ap_num=0 records_returned=0`
  - wrapped Rust broad: `scan=ok elapsed_ms=182 result_count=0`

Conclusion:
- explicit RF hold during the failing boot-scan window does not restore packet visibility or scan results
- this closes the “radio is simply sleeping / RF closed during the no-std boot scan” branch
- the remaining target stays in earliest no-std RX ingress or admission before any BSS/history side effects

## 2026-03-09: forcing `wifi_task_core_id` to the IDF default core does not restore RX visibility

- Reused the existing vendored `esp-radio` init-config gate:
  - `MEDITAMER_WIFI_ESP_RADIO_USE_IDF_TASK_CORE=1`
- This forces `wifi_init_config_t.wifi_task_core_id` to the IDF default core (`WIFI_TASK_CORE_ID`) instead of the no-std default `Cpu::current()`.
- Rebuilt the env-gated boot-scan app with:
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_PROMISC_DIAG=1`
  - `MEDITAMER_WIFI_EARLY_DRIVER_STATE_DIAG=1`
  - `MEDITAMER_WIFI_ESP_RADIO_USE_IDF_TASK_CORE=1`
- Flashed app-only to `0x10000` via `esptool.py write_flash` and captured:
  - `logs/boot_scan_only_diag_idftaskcore_20260309_092559/boot_espflash_monitor.log`

Key evidence from the log:
- The pre-scan promisc window remained fully dark:
  - `boot_scan_only_promisc_diag outcome=ok ... total=0 mgmt=0 ctrl=0 data=0 misc=0`
- Raw `ScanDone` list state remained empty for all three scan variants:
  - `scan_id=128 ... scannum=0x0000 head_ptr=0x0`
  - `scan_id=129 ... scannum=0x0000 head_ptr=0x0`
  - `scan_id=130 ... scannum=0x0000 head_ptr=0x0`
- All three scan entry paths still failed:
  - direct IDF `NULL`: `idf_compare=ok ... ap_num=0 records_returned=0`
  - direct IDF explicit broad: `idf_explicit_compare=ok ... ap_num=0 records_returned=0`
  - wrapped Rust broad: `scan=ok elapsed_ms=178 result_count=0`

Conclusion:
- pinning the Wi-Fi task to the IDF default core does not restore RX visibility or scan results
- this closes the “Wi-Fi task core placement is the primary cause” branch
- the remaining credible boundary stays below task placement, in deeper no-std RX ingress/runtime behavior

## 2026-03-09: the failing no-std scan path allocates through Wi-Fi OS hooks, but raw `ScanDone` still has an empty BSS list

- Added allocator counters in the vendored no-std Wi-Fi OS adapter:
  - `vendor/esp-radio-0.17.0/src/wifi/os_adapter/mod.rs`
  - `src/firmware/storage/upload/wifi/connect/boot_scan_diag.rs`
- The probe records allocation and free activity across the same three failing scan entry variants:
  - direct IDF `NULL` scan
  - direct IDF explicit broad scan
  - wrapped Rust broad scan
- Rebuilt the same timing-normalized boot-only image, merged it, and reflashed via `esptool.py write_flash`.
- Captured bounded log:
  - `logs/boot_scan_only_diag_alloc_20260309_berlin/boot_espflash_monitor.log`

Key evidence from the log:
- Raw `ScanDone` list state is still empty for all three variants:
  - `scan_id=128 scannum=0x0000 head_ptr=0x0 tail_ptr=0x3ffd3308`
  - `scan_id=129 scannum=0x0000 head_ptr=0x0 tail_ptr=0x3ffd3308`
  - `scan_id=130 scannum=0x0000 head_ptr=0x0 tail_ptr=0x3ffd3308`
- Yet the same failing window clearly exercises Wi-Fi allocation hooks:
  - after direct IDF `NULL` scan:
    - `malloc_internal_count=26 total=2392 max=176`
    - `wifi_malloc_count=13 total=104 max=8`
    - `wifi_calloc_count=11 total=1948 max=952`
    - `free_count=35`
  - after direct IDF explicit broad scan:
    - `malloc_internal_count=52 total=4784`
    - `wifi_malloc_count=26 total=208`
    - `wifi_calloc_count=12 total=2016`
    - `free_count=62`
  - after wrapped Rust broad scan:
    - `malloc_internal_count=78 total=7176`
    - `wifi_malloc_count=39 total=312`
    - `wifi_calloc_count=14 total=2108`
    - `free_count=90`

Conclusion:
- the failing no-std scan path is not dead before all Wi-Fi memory activity
- but the raw `ScanDone` event still sees an empty BSS/result list
- this tightens the remaining target further:
  - either candidate/BSS records are rejected or freed before they are linked into the result list
  - or the observed allocation activity belongs to adjacent scan machinery while beacon/probe admission never reaches list insertion

## 2026-03-07: the BSS/result list is already empty at raw `ScanDone`, not just later at `get_ap_num`

- Added a new event-boundary probe:
  - `src/firmware/storage/upload/wifi/connect/events.rs`
  - `src/firmware/storage/upload/wifi/connect/blob_state_diag.rs`
- The probe logs, at the raw `event::ScanDone` callback before later host-side comparisons:
  - `scannum`
  - `g_ic + 0x130` (list head)
  - `g_ic + 0x134` (tail/sentinel slot)
- Rebuilt the same timing-normalized boot-only image and reflashed via merged image plus
  `esptool.py write_flash`.
- Captured bounded log:
  - `logs/boot_scan_only_diag_scandonelist_20260307_berlin/boot_espflash_monitor.log`

Key evidence from the log:
- At the raw `ScanDone` boundary for all three failing scan variants:
  - direct IDF `NULL` scan:
    - `event scan_done_list status=0 count=0 scan_id=128 scannum=0x0000 head_ptr=0x0 tail_ptr=0x3ffd32b0`
  - direct IDF explicit broad scan:
    - `event scan_done_list status=0 count=0 scan_id=129 scannum=0x0000 head_ptr=0x0 tail_ptr=0x3ffd32b0`
  - wrapped Rust broad scan:
    - `event scan_done_list status=0 count=0 scan_id=130 scannum=0x0000 head_ptr=0x0 tail_ptr=0x3ffd32b0`
- The later checkpoints stay consistent with that:
  - `scan_done_eventpost ... ap_num=0`
  - `esp_wifi_scan_get_ap_num -> 0`
  - wrapped Rust `result_count=0`

Conclusion:
- this is the strongest result-population narrowing so far:
  - the no-std path does not appear to populate the BSS/result list at all in the failing runs
  - the failure is therefore not “records exist and get cleared before `get_ap_num`”
- the root-cause boundary moves below result retrieval and list cleanup:
  - either scan/RX never produces candidate records into the list
  - or blob-internal filtering rejects them before they ever enter the list

## 2026-03-09: forcing an extra `phy_enable()` reference does not restore RX visibility

- Added boot-scan-only PHY reference diagnostics in:
  - `vendor/esp-radio-0.17.0/src/wifi/os_adapter/mod.rs`
  - `vendor/esp-radio-0.17.0/src/lib.rs`
  - `src/firmware/storage/upload/wifi/connect/boot_scan_diag.rs`
- The new diagnostic gate is:
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_FORCE_PHY_ENABLE_DIAG=1`
- The probe now:
  - counts blob-side `phy_enable()` / `phy_disable()` hook calls in the no-std Wi-Fi OS adapter
  - invokes one extra `phy_enable()` reference immediately after `start=ok`
  - holds that reference across the pre-scan promisc sweep, direct IDF `NULL` scan, direct IDF explicit broad scan, and wrapped Rust broad scan
  - releases the extra reference before `stop_async()`
- Rebuilt the env-gated boot-scan app, flashed app-only to `0x10000` via `esptool.py write_flash`, and captured:
  - `logs/boot_scan_only_diag_forcephy_20260309_093237/boot_espflash_monitor.log`

Key evidence from the log:
- The extra PHY reference path definitely ran:
  - `force_phy_enable invoked=true`
  - `force_phy_disable invoked=true`
- The adapter counters show the hook was exercised twice by the time the first direct IDF compare completes:
  - after `idf_compare_first`: `phy_enable_count=2 phy_disable_count=0`
  - after `idf_explicit_compare_first`: `phy_enable_count=2 phy_disable_count=0`
  - after `rust_scan`: `phy_enable_count=2 phy_disable_count=0`
- Despite that, the pre-scan promisc window remained fully dark:
  - `boot_scan_only_promisc_diag outcome=ok ... total=0 mgmt=0 ctrl=0 data=0 misc=0`
- Raw `ScanDone` list state remained empty for all three scan variants:
  - `scan_id=128 ... scannum=0x0000 head_ptr=0x0`
  - `scan_id=129 ... scannum=0x0000 head_ptr=0x0`
  - `scan_id=130 ... scannum=0x0000 head_ptr=0x0`
- All three scan entry paths still failed:
  - direct IDF `NULL`: `idf_compare=ok ... ap_num=0 records_returned=0`
  - direct IDF explicit broad: `idf_explicit_compare=ok ... ap_num=0 records_returned=0`
  - wrapped Rust broad: `scan=ok elapsed_ms=178 result_count=0`

Conclusion:
- adding an extra no-std `phy_enable()` reference does not restore packet visibility or scan results
- this closes the “missing PHY enable reference is the primary cause” branch
- the remaining boundary is now deeper than explicit RF wake, task placement, common PHY clock refcounting, timer-task precreation, and manual PHY enable reference
