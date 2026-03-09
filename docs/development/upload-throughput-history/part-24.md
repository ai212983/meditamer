# Upload Throughput History Part 24

## 2026-03-09: exposed the new `esp-rtos` bootstrap shim through `backend_legacy_port`

- Promoted the first lower-layer legacy-port helper from vendored runtime support into the firmware staging module.
- Added:
  - `src/firmware/storage/upload/wifi/backend_legacy_port/bootstrap.rs`
    - `LegacyBootstrapRuntimeStatus`
    - `runtime_bootstrap_status()`
- This now calls:
  - `esp_rtos::bootstrap_legacy_wifi_contract_shim()`
- The vendored shim currently provides the closest current-runtime approximation of three legacy bootstrap expectations:
  - scheduler is already initialized
  - timer task gets precreated
  - one initial yield is performed

Validation:
- `cargo check` passes after adding the shim export and firmware-local status wrapper.

Why this matters:
- the legacy-port path now has one executable lower-layer hook instead of only notes about what is missing
- this is still not the real legacy bootstrap contract
- the remaining structural blocker is explicit:
  - no direct current equivalent for legacy `preempt::enable()`
  - no direct current equivalent for legacy `init_tasks()`

Conclusion:
- further progress is no longer seam cleanup
- the next meaningful increment must modify or port runtime behavior below the firmware backend seam

## 2026-03-09: inserted the bootstrap shim at the real `esp_radio::init()` boundary and rejected it as sufficient

- Moved the first real runtime-port experiment into:
  - `vendor/esp-radio-0.17.0/src/lib.rs`
- New guarded knob:
  - `MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_BOOTSTRAP_SHIM_DIAG=1`
- The shim now runs at the closest legacy-equivalent spot in current init:
  - after `setup_radio_isr()`
  - before `wifi_set_log_verbose()` and `init_radio_clocks()`
- Added runtime-visible shim diagnostics to the isolated current standalone comparator:
  - `tools/esp_radio_nostd_wifi_control/src/main.rs`

Validation:
- `cargo check --manifest-path tools/esp_radio_nostd_wifi_control/Cargo.toml` passed.
- Built and flashed the isolated current standalone comparator with the knob enabled.
- Live PTY summary:
  - `logs/esp_radio_nostd_wifi_control_legacybootstrapshim_20260309_berlin/pty_summary.txt`

Observed result:
- startup still reached:
  - `begin=true`
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `start=ok`
- scan still ended at:
  - `scan=ok count=0`
- late queue-family behavior still matched prior failing current-stack runs

Conclusion:
- inserting the shim at the real `esp_radio::init()` boundary is not sufficient to restore RX visibility
- this closes the narrow “precreate+yield shim at current init time is enough” branch
- the remaining blocker is still deeper runtime behavior than this shim provides

## 2026-03-09: strengthened the runtime bootstrap shim to wait for timer-task entry and rejected it as well

- Tightened the same vendored runtime-port experiment by changing the shim to verify that the `timer` task actually entered its loop.
- Changes:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/timer_queue.rs`
    - added `TIMER_TASK_ENTRY_COUNT`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/bootstrap.rs`
    - shim now yields up to `8` times and waits for `timer_task_entry_count()` to advance
  - `vendor/esp-radio-0.17.0/src/lib.rs`
    - extended bootstrap-shim diagnostics with `timer_task_started`
  - `tools/esp_radio_nostd_wifi_control/src/main.rs`
    - extended isolated-tool reporting for the stronger shim

Validation:
- `cargo check --manifest-path tools/esp_radio_nostd_wifi_control/Cargo.toml` passed.
- Built and flashed the isolated current standalone comparator.
- Live PTY summary:
  - `logs/esp_radio_nostd_wifi_control_legacybootstrapshim2_20260309_berlin/pty_summary.txt`

Observed result:
- startup still reached:
  - `begin=true`
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `start=ok`
- scan still ended at:
  - `scan=ok count=0`
- queue/task and late queue-family behavior remained equivalent to the earlier weaker shim run

Conclusion:
- even when the shim is strengthened to wait for actual timer-task entry, current `esp-radio` still stays RX-dark and scans zero
- this closes the stronger “legacy bootstrap can be recovered by making the timer task definitely run early” branch
- the remaining problem is still deeper than bootstrap-precreate/yield semantics

## 2026-03-09: forced current `_esp_timer_get_time` onto the legacy HAL-time source and rejected it as sufficient

- Compared the remaining concrete wrapper-level deltas between:
  - working legacy `esp-wifi 0.15.1`
  - failing current vendored `esp-radio 0.17.0`
- The first novel runtime-facing delta worth testing was `_esp_timer_get_time`:
  - current `esp-radio`: `__esp_radio_esp_timer_get_time()` normally returns `preempt::now()`
  - legacy path uses a raw HAL/systimer-backed microsecond source instead of scheduler time for the corresponding timer wrapper path
- Added a guarded current-stack A/B in:
  - `vendor/esp-radio-0.17.0/src/common_adapter.rs`
- New knob:
  - `MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_ESP_TIMER_GET_TIME_DIAG=1`
- With the knob enabled, current `__esp_radio_esp_timer_get_time()` now returns:
  - `esp_hal::time::Instant::now().duration_since_epoch().as_micros() as i64`
  instead of scheduler-backed `preempt::now()`

Validation:
- `cargo check --manifest-path tools/esp_radio_nostd_wifi_control/Cargo.toml` passed.
- Built and flashed the isolated current standalone comparator.
- Captured:
  - `logs/esp_radio_nostd_wifi_control_legacytime_20260309_berlin/monitor.log`

Observed result:
- startup still reached:
  - `begin=true`
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `start=ok`
- scan still ended at:
  - `scan=ok count=0`
- queue-role and late queue-family behavior still matched prior failing isolated current runs

Conclusion:
- switching current `_esp_timer_get_time` from scheduler time to the legacy-style HAL time source is not sufficient to recover RX visibility
- this closes the remaining novel timer-source wrapper branch
- the strongest remaining deltas are now below simple wrapper substitution and above a full legacy-runtime/backend port

## 2026-03-09: validated the unified `backend_legacy_port` firmware path and rejected it as sufficient

- Built the main firmware with the new unified legacy-port knob:
  - `MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_PROMISC_DIAG=1`
- Reused the now-proven reliable flash path for large debug images:
  - `cargo build`
  - `espflash save-image --chip esp32 ... app.bin`
  - `esptool.py --no-stub --baud 115200 write_flash -z 0x10000 app.bin`
- Captured a bounded raw boot window by opening `/dev/cu.usbserial-510` directly, pulsing modem lines on the same file descriptor, and reading the resulting boot log into:
  - `logs/boot_scan_backend_legacy_port_20260309_172022/monitor.log`

Observed result:
- the unified path was definitely active:
  - `wifi_backend name=esp-radio`
  - `legacy_port_runtime name=backend-legacy-port ...`
  - `legacy_port_bootstrap scheduler_initialized=true timer_task_precreated=true timer_task_started=true yielded_once=true`
  - `legacy_port runtime_init result=ok`
- despite that, the pre-scan boot-only promisc window remained fully dark:
  - `boot_scan_only_promisc_diag ... total=0 mgmt=0 ctrl=0 data=0 misc=0`
- direct IDF `NULL` scan still returned zero:
  - `boot_scan_only_diag idf_compare=ok ... ap_num=0`
- direct IDF explicit broad scan still returned zero:
  - `boot_scan_only_diag idf_explicit_compare=ok ... ap_num=0`
- wrapped backend scan still returned zero:
  - `boot_scan_only_diag scan=ok elapsed_ms=205 result_count=0`
- the raw `ScanDone` list and admission state remained empty through the same path:
  - `event scan_done_list ... scannum=0x0000 head_ptr=0x0`

Conclusion:
- the new unified `backend_legacy_port` path is wired correctly and executes at runtime
- but the current implementation is still not sufficient to recover RX visibility or scan admission
- this closes the branch “legacy-port runtime shim + legacy-style sync `start/scan/stop` is enough”
- the next meaningful work is the deeper source-level legacy runtime/backend port, not more validation reruns of this same knob

## 2026-03-09: made `backend_legacy_port` an executable firmware path instead of staging-only

- Added a real in-tree legacy-port runtime path:
  - `src/firmware/storage/upload/wifi/backend_legacy_port/runtime.rs`
- Threaded that path into firmware runtime init:
  - `src/firmware/storage/upload/wifi/runtime_init.rs`
- Extended the same legacy-port knob so it also governs controller semantics:
  - `src/firmware/storage/upload/wifi/backend.rs`

What changed:
- New knob:
  - `MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG=1`
- When enabled, firmware runtime init now routes through:
  - `backend_legacy_port::initialize_runtime_sta_legacy_port(...)`
- That path now:
  - logs the legacy bootstrap contract
  - logs current runtime shim status (`scheduler_initialized`, `timer_task_precreated`, `timer_task_started`, `yielded_once`)
  - performs the actual `init_radio() -> new_runtime(...)` bring-up through the legacy-port module
- The same knob now also enables legacy-style synchronous controller `start/scan/stop` semantics via the backend seam, rather than leaving those on a separate ad hoc path.

Why this matters:
- `backend_legacy_port` is no longer just a staging folder with contracts and blocker notes.
- There is now one coherent executable firmware path for the legacy-port work:
  - runtime bootstrap/init
  - controller `start/scan/stop`
- This is the first code step that makes the legacy-port path end-to-end selectable from firmware without patching the upload state machine again.

Validation:
- `cargo check` passed after wiring the runtime path and controller path together.

Conclusion:
- the migration is now beyond seam cleanup
- the next meaningful increment is live validation of this unified `backend_legacy_port` path on hardware
