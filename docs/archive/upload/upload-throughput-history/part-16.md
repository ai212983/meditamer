# Upload Throughput History Part 16

## 2026-03-06: official ESP-IDF still scans with `wifi_init_config_t.nvs_enable = 0`

- Added a narrow C-control probe knob to disable Wi-Fi NVS during `esp_wifi_init()`:
  - `tools/esp_idf_wifi_control/main/Kconfig.projbuild`
  - `tools/esp_idf_wifi_control/main/wifi_control_main.c`
  - `tools/esp_idf_wifi_control/sdkconfig.nvs_off.defaults`
- The probe now logs `wifi_init nvs_enable=%d` before `esp_wifi_init(&cfg)`.
- Built and flashed a separate scan-only control image against external ESP-IDF
  `v5.5.2` with:
  - `SDKCONFIG_DEFAULTS=tools/esp_idf_wifi_control/sdkconfig.nvs_off.defaults`
  - build dir `/.embuild/idf_apps/wifi_control_nvs_off/build`
- Captured bounded log:
  - `logs/esp_idf_wifi_control_nvs_off_20260306_115003/monitor.log`

Key evidence from the log:
- `wifi_control: wifi_init nvs_enable=0`
- `wifi:config NVS flash: disabled`
- `wifi_control: mode=scan_only scan_list_size=10`
- `wifi_control: scan_complete total_ap_count=7 returned_ap_count=7`
- scanned APs still include:
  - `<test-ssid-primary>`
  - `<test-ssid-guest>`
  - `<nearby-ssid-1>`
  - `<nearby-ssid-3>`

Conclusion:
- disabling Wi-Fi NVS in official ESP-IDF does **not** reproduce the blackout
  on this board/network
- therefore `nvs_enable=0` is not a sufficient explanation for the current
  `esp-radio` no-scan state
- the earlier `esp-radio` panic when forcing IDF-like init defaults is still
  relevant, but it indicates an adapter limitation (`nvs_open` unimplemented),
  not the root cause of zero-scan behavior

Most likely remaining boundary:
- `esp-radio` OS-adapter / task / event integration after `sta_start`, not the
  simple fact that Wi-Fi NVS is disabled

## 2026-03-06: Wi-Fi `is_from_isr()` real-context A/B did not restore scan visibility

- Added a guarded `esp-radio` Wi-Fi OS-adapter experiment in:
  - `vendor/esp-radio-0.17.0/src/wifi/os_adapter/mod.rs`
- New build-time knob:
  - `MEDITAMER_WIFI_ESP_RADIO_USE_REAL_ISR_CHECK=1`
- The experiment switches Wi-Fi `is_from_isr()` from the existing unconditional
  `true` return to `crate::is_interrupts_disabled()`, matching the internal
  ISR-context check already used on the BLE side of `esp-radio`.
- Rebuilt and flashed the main firmware with:
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
  - `MEDITAMER_WIFI_ESP_RADIO_USE_REAL_ISR_CHECK=1`
- Captured bounded boot log:
  - `logs/boot_scan_only_diag_realisr_20260306_120359/boot_espflash_monitor.log`

Key evidence from the log:
- `upload_http: event sta_start`
- `upload_http: boot_scan_only_diag start=ok`
- `upload_http: event scan_done status=0 count=0 scan_id=128`
- `upload_http: boot_scan_only_diag scan=ok elapsed_ms=178 result_count=0`
- `upload_http: event scan_done status=0 count=0 scan_id=129`
- `upload_http: boot_scan_only_diag idf_compare=ok ... ap_num=0`

Conclusion:
- the unconditional `is_from_isr() -> true` adapter behavior is suspicious and
  semantically wrong on its face, but correcting it in this bounded A/B did
  **not** restore AP visibility
- therefore it is not the dominant root cause of the current blackout
- the remaining fault boundary stays in broader `esp-radio` OS-adapter /
  runtime integration, not this single ISR-context predicate

## 2026-03-06: IDF-like `wifi_init_config_t` defaults without NVS still did not recover scans

- Added another guarded `esp-radio` init-config experiment in:
  - `vendor/esp-radio-0.17.0/src/wifi/mod.rs`
- New build-time knob:
  - `MEDITAMER_WIFI_ESP_RADIO_USE_IDF_INIT_DEFAULTS_NO_NVS=1`
- This applies the remaining IDF-like init defaults already available from
  `esp_wifi_sys` while explicitly keeping `nvs_enable=0`, to avoid the known
  `nvs_open` panic in the no-std adapter path.
- Rebuilt and flashed the main firmware with:
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
  - `MEDITAMER_WIFI_ESP_RADIO_USE_IDF_INIT_DEFAULTS_NO_NVS=1`
- Captured bounded boot log:
  - `logs/boot_scan_only_diag_idfdefaults_nonvs_20260306_120359/boot_espflash_monitor.log`

Key evidence from the log:
- `upload_http: event sta_start`
- `upload_http: boot_scan_only_diag start=ok`
- `upload_http: event scan_done status=0 count=0 scan_id=128`
- `upload_http: boot_scan_only_diag scan=ok elapsed_ms=177 result_count=0`
- `upload_http: event scan_done status=0 count=0 scan_id=129`
- `upload_http: boot_scan_only_diag idf_compare=ok ... ap_num=0`

Conclusion:
- adopting the rest of the IDF-style `wifi_init_config_t` defaults, while
  avoiding the NVS adapter trap, still did **not** restore scan visibility
- this further weakens the theory that the blackout is driven by simple
  init-config field deltas
- the dominant remaining target is now runtime task / interrupt / OS-adapter
  behavior after `sta_start`, not init-config selection

## 2026-03-06: failing baseline still receives `WIFI_MAC` interrupts during zero-result scans

- Added a minimal `WIFI_MAC` ISR entry counter in vendored `esp-radio`:
  - `vendor/esp-radio-0.17.0/src/radio/radio_esp32.rs`
  - exported via hidden diagnostics in `vendor/esp-radio-0.17.0/src/lib.rs`
- Wired the counter into the boot-only scan repro in:
  - `src/firmware/storage/upload/wifi/connect/boot_scan_diag.rs`
- Rebuilt and flashed the plain failing baseline with:
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
- Captured bounded boot log:
  - `logs/boot_scan_only_diag_isrcount_20260306_120359/boot_espflash_monitor.log`

Key evidence from the log:
- `upload_http: boot_scan_only_diag scan=ok elapsed_ms=179 result_count=0`
- `upload_http: boot_scan_only_diag wifi_mac_isr_count after=rust_scan count=27`
- `upload_http: boot_scan_only_diag idf_compare=ok ... ap_num=0`
- `upload_http: boot_scan_only_diag wifi_mac_isr_count after=idf_compare count=53`

Conclusion:
- the blackout is **not** explained by a dead `WIFI_MAC` interrupt path
- radio/Wi-Fi interrupts are firing during the failing window, yet both the
  Rust scan path and the direct IDF scan comparator still produce zero APs
- this shifts the primary target upward from raw interrupt delivery to:
  - interrupt-to-driver work handoff
  - scan result population / event handoff
  - broader OS-adapter runtime semantics after ISR entry

## 2026-03-06: failing baseline still shows active Wi-Fi OS queue/semaphore handoff

- Added compact Wi-Fi OS-adapter counters in vendored `esp-radio`:
  - `vendor/esp-radio-0.17.0/src/common_adapter.rs`
  - hidden accessors exported from `vendor/esp-radio-0.17.0/src/lib.rs`
- Wired them into the boot-only scan repro in:
  - `src/firmware/storage/upload/wifi/connect/boot_scan_diag.rs`
- Rebuilt and flashed the plain failing baseline with:
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
- Captured bounded boot log:
  - `logs/boot_scan_only_diag_osdiag_20260306_120359/boot_espflash_monitor.log`

Key evidence from the log:
- after the first zero-result Rust scan:
  - `wifi_mac_isr_count ... count=27`
  - `wifi_os_diag ... sem_take=5 sem_give=5 queue_send=44 queue_send_isr=14 queue_recv=58 event_post=0`
- after the direct-IDF zero-result compare:
  - `wifi_mac_isr_count ... count=53`
  - `wifi_os_diag ... sem_take=6 sem_give=6 queue_send=84 queue_send_isr=27 queue_recv=111 event_post=0`

Conclusion:
- the failing baseline is not frozen at the Wi-Fi OS-adapter handoff layer:
  - interrupts fire
  - queue send/receive traffic happens
  - semaphore take/give traffic happens
- despite that active runtime movement, both scan paths still complete with zero
  APs
- this pushes the remaining root-cause boundary tighter toward:
  - packet/RF receive path rather than generic queue starvation
  - scan result generation/classification rather than event wakeup only

## 2026-03-06: failing baseline still shows zero STA/AP RX callback activity

- Added internal STA/AP RX callback counters in vendored `esp-radio`:
  - `vendor/esp-radio-0.17.0/src/wifi/mod.rs`
- Wired them into the boot-only scan repro in:
  - `src/firmware/storage/upload/wifi/connect/boot_scan_diag.rs`
- Rebuilt and flashed the plain failing baseline with:
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
- Captured bounded boot log:
  - `logs/boot_scan_only_diag_rxcb_20260306_120359/boot_espflash_monitor.log`

Key evidence from the log:
- after the first zero-result Rust scan:
  - `wifi_mac_isr_count ... count=27`
  - `wifi_rx_cb_count ... sta=0 ap=0`
  - `wifi_os_diag ... queue_send=44 queue_send_isr=14 queue_recv=58`
- after the direct-IDF zero-result compare:
  - `wifi_mac_isr_count ... count=53`
  - `wifi_rx_cb_count ... sta=0 ap=0`
  - `wifi_os_diag ... queue_send=84 queue_send_isr=27 queue_recv=111`

Conclusion:
- during the failing boot-scan window:
  - Wi-Fi interrupts fire
  - OS queue/semaphore traffic exists
  - but the registered STA/AP internal RX callbacks never fire
- this is useful narrowing, with one caveat: these registered STA/AP callbacks
  may primarily represent the post-association data path rather than the scan
  result path itself
- so the result does **not** prove the scan engine must pass through these
  callbacks
- it does still support the broader picture that:
  - the runtime is alive
  - interrupts and OS handoff are active
  - yet scan/result visibility remains absent

## 2026-03-06: `ScanDone` event-post snapshot already reports zero APs

- Added a direct `ScanDone` event-post diagnostic snapshot in vendored
  `esp-radio`:
  - `vendor/esp-radio-0.17.0/src/wifi/os_adapter/mod.rs`
  - hidden accessors exported from `vendor/esp-radio-0.17.0/src/lib.rs`
- Wired that snapshot into the boot-only scan repro in:
  - `src/firmware/storage/upload/wifi/connect/boot_scan_diag.rs`
- Rebuilt and flashed the plain failing baseline with:
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
- Captured bounded boot log:
  - `logs/boot_scan_only_diag_eventpostdiag_20260306_120359/boot_espflash_monitor.log`

Key evidence from the log:
- first scan:
  - `event scan_done status=0 count=0 scan_id=128`
  - `scan_done_eventpost after=rust_scan count=1 status=0 number=0 scan_id=128 ap_num_rc=0 ap_num=0`
- direct IDF compare:
  - `event scan_done status=0 count=0 scan_id=129`
  - `scan_done_eventpost after=idf_compare count=2 status=0 number=0 scan_id=129 ap_num_rc=0 ap_num=0`

Conclusion:
- zero visibility is already present at the driver-facing `ScanDone` event-post
  boundary
- it is **not** being introduced later by:
  - `scan_results()`
  - `esp_wifi_scan_get_ap_record`
  - the firmware-side event logger
- this is the strongest current narrowing:
  - interrupts are active
  - OS handoff is active
  - but the Wi-Fi stack itself posts scan completion with zero AP count in the
    `esp-radio` no-std path

## 2026-03-06: first-scan `esp_wifi_scan_start(NULL, true)` still returns zero in the failing runtime

- Added an ordering A/B knob to the boot-only scan repro:
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST=1`
- This runs the direct IDF null-config scan first, before the normal
  `esp-radio` scan wrapper, inside the same failing runtime.
- Rebuilt and flashed with:
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST=1`
- Captured bounded boot log:
  - `logs/boot_scan_only_diag_idfnullfirst_20260306_120359/boot_espflash_monitor.log`

Key evidence from the log:
- first scan in runtime was direct IDF null-config:
  - `boot_scan_only_diag idf_null_first begin=true`
  - `event scan_done status=0 count=0 scan_id=128`
  - `idf_compare=ok ... ap_num=0`
  - `scan_done_eventpost ... number=0 ... ap_num=0`
- the following normal `esp-radio` scan also remained zero:
  - `event scan_done status=0 count=0 scan_id=129`
  - `scan=ok ... result_count=0`

Conclusion:
- the blackout is **not** caused by the explicit `esp-radio` scan wrapper
  fields such as:
  - `show_hidden=true`
  - `home_chan_dwell_time=0`
  - explicit `wifi_scan_config_t` construction
- even the first `esp_wifi_scan_start(NULL, true)` in the failing no-std
  runtime returns zero APs
- that further tightens the boundary to the surrounding `esp-radio` runtime /
  driver bring-up state, not the scan-config wrapper

## 2026-03-06: working IDF controls match each other, failing `esp-radio` path differs pre-scan

- Added identical `pre_scan_driver_state` logging to:
  - `tools/esp_idf_wifi_control/main/wifi_control_main.c`
  - `tools/esp_idf_wifi_control_rust/src/main.rs`
  - `src/firmware/storage/upload/wifi/connect/boot_scan_diag.rs`
- Rebuilt and captured:
  - `logs/esp_idf_wifi_control_scan_state_20260306_berlin.log`
  - `logs/esp_idf_wifi_control_rust_scan_state_20260306_berlin.log`
  - `logs/boot_scan_only_diag_statecmp_20260306_berlin.log`

Key evidence:
- working C / ESP-IDF:
  - `pre_scan_driver_state ... mode=1 ... ps=1 ... max_tx_power=78 ... protocol_bitmap=0x07 ... cc=01. ... policy=0 ... scan_active_max=120 ... scan_passive=360 ...`
- working Rust-on-IDF:
  - `pre_scan_driver_state ... mode=1 ... ps=1 ... max_tx_power=78 ... protocol_bitmap=0x07 ... cc=01. ... policy=0 ... scan_active_max=120 ... scan_passive=360 ...`
- failing no-std `esp-radio` boot repro:
  - `boot_scan_only_driver_state ... mode=1 ... ps=0 ... max_tx_power=80 ... protocol_bitmap=0x07 ... cc=CN. ... policy=1 ... scan_active_max=120 ... scan_passive=360 ...`

Conclusion:
- the working IDF controls agree with each other on the visible pre-scan driver
  state
- the failing `esp-radio` runtime differs before the first scan in at least:
  - power save: `ps=0` vs `ps=1`
  - country/policy: `CN` manual vs `01` auto
  - max TX power: `80` vs `78`
- the scan defaults themselves match, so the next highest-value step is a
  causality test on the visible deltas, not more generic tracing
- the concrete next A/B is:
  - force the working IDF control app to `WIFI_PS_NONE` and the same country
    policy as the failing path
  - or conversely force the `esp-radio` path toward the working control values
  - then rerun the same bounded first-scan comparison


_Continued in [Part 16, continuation 2](./part-16-02.md)._
