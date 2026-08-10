# Upload Throughput History, Part 23

## 2026-03-09: introduced a firmware Wi-Fi backend seam while keeping `esp-radio` active

- Added a dedicated backend module:
  - `src/firmware/storage/upload/wifi/backend.rs`
- Moved the low-level runtime entrypoints behind backend wrappers:
  - `init_radio()`
  - `new_runtime(...)`
- Re-exported the current Wi-Fi controller/device types through that backend layer so the existing upload/connect code keeps its original lifetime-bearing type shape.
- Rewired firmware setup to go through the backend layer:
  - `src/firmware/storage/upload/mod.rs`
  - `src/firmware/storage/upload/wifi.rs`
  - dependent upload Wi-Fi modules now import backend-owned types instead of reaching directly into `esp_radio` at the setup boundary.
- Centralized controller operations behind backend wrappers:
  - `wifi_set_config`
  - `wifi_set_mode`
  - `wifi_is_started`
  - `wifi_set_power_saving`
  - `wifi_set_protocol`
  - `wifi_rssi`
  - `wifi_scan_with_config_async`
  - `wifi_start_async`
  - `wifi_stop_async`
  - `wifi_connect_async`
  - `wifi_disconnect_async`
- Replaced upload-path direct controller method calls with those backend wrappers, so the future backend swap no longer has to touch the Wi-Fi state machine everywhere.
- Added a backend compatibility knob that reproduces the working legacy standalone start/scan sequence using the current backend surface:
  - `MEDITAMER_WIFI_BACKEND_LEGACY_SYNC_START_SCAN_DIAG=1`
  - when enabled, backend wrappers use synchronous `start`, `scan_with_config`, and `stop` instead of the current async event-waiting variants
  - this keeps call sites unchanged while allowing a direct A/B against the legacy no-std control logic inside the main firmware crate

Validation:
- `cargo check` passes after the seam rewrite.
- Firmware setup now logs the selected backend explicitly:
  - `upload_http: wifi_backend name=esp-radio`
- Cargo now exposes backend selection explicitly:
  - `wifi-backend-esp-radio`

Constraint discovered during this step:
- adding `esp-wifi 0.15.1` directly as an optional dependency in the main firmware crate is not currently viable
- Cargo resolution fails before build because `esp-wifi 0.15.1` pulls `xtensa-lx-rt 0.20.0`, which conflicts with the main firmware's `esp-hal 1.0.0` path on `xtensa-lx-rt 0.21.0`
- that means the real `esp-wifi` backend implementation likely needs either:
  - a vendored/ported backend layer adapted to the current runtime stack, or
  - a separate crate boundary instead of a direct optional dependency in the main firmware crate

Why this matters:
- this is the first migration step toward swapping the low-level Wi-Fi implementation without rewriting the upload state machine
- it keeps current behavior unchanged while isolating the exact surface that an `esp-wifi` backend will need to implement next

Conclusion:
- the codebase now has a build-clean backend seam
- current backend selection remains `esp-radio`
- controller call-sites are now funneled through backend-owned wrappers instead of direct `esp-radio` methods
- next work should implement the first real alternative backend behind this seam, most likely by:
  - porting the working legacy scan/start path into a vendored backend module or separate crate boundary
  - then swapping `init/new/start/scan/stop` behind `backend.rs`
- immediate live next step:
  - flash a firmware build with `MEDITAMER_WIFI_BACKEND_LEGACY_SYNC_START_SCAN_DIAG=1`
  - rerun the isolated pre-scan promisc + scan checks to see whether legacy synchronous start/scan semantics restore RX visibility inside the main firmware path

## 2026-03-09: rejected legacy synchronous backend start/scan/stop as a sufficient firmware fix

- Built and flashed a main-firmware image with:
  - `MEDITAMER_WIFI_BACKEND_LEGACY_SYNC_START_SCAN_DIAG=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_COMPARE=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE=1`
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_PROMISC_DIAG=1`
- Flash succeeded via the reliable app-only `esptool.py write_flash ... 0x10000` path.
- A bounded `espflash` monitor capture produced a valid runtime log at:
  - `logs/boot_scan_legacysync_20260309_150800/monitor_espflash.log`

Key result:
- backend selection stayed on the current implementation:
  - `upload_http: wifi_backend name=esp-radio`
- pre-scan promisc remained completely dark:
  - channels `8/1/6/11`: all `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - aggregate: `total=0`
- wrapped boot scan still failed:
  - `scan_done status=0 count=0 scan_id=128`
  - `boot_scan_only_diag scan=ok elapsed_ms=205 result_count=0`
- direct IDF `NULL` scan still failed:
  - `scan_done status=0 count=0 scan_id=129`
  - `boot_scan_only_diag idf_compare=ok ... ap_num=0`
- direct IDF explicit broad scan still failed:
  - `scan_done status=0 count=0 scan_id=130`
  - `boot_scan_only_diag idf_explicit_compare=ok ... ap_num=0`

Conclusion:
- reproducing the working legacy standalone's synchronous `start/scan/stop` controller semantics inside the main firmware backend is not sufficient to restore RX visibility
- this closes the “legacy synchronous controller call style alone fixes the current firmware blackout” branch
- the remaining difference is earlier and deeper than async-vs-sync controller sequencing

## 2026-03-09: rejected skipping `set_mac_time_update_cb()` as the isolated current-stack fix

- Ran the isolated current standalone comparator with:
  - `MEDITAMER_WIFI_ESP_RADIO_SKIP_MAC_TIME_UPDATE_CB_DIAG=1`
- Built with the explicit Xtensa toolchain on `PATH`, flashed app-only to `0x10000`, and captured a bounded PTY monitor log at:
  - `logs/esp_radio_nostd_wifi_control_skipmactime_20260309_berlin/monitor.log`

Key result:
- startup stayed clean:
  - `begin=true`
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `start=ok`
- the pre-scan promisc window remained fully dark:
  - channels `8/1/6/11`: all `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - aggregate: `total=0`
- scan still ended at:
  - `scan=ok count=0`
- queue/runtime shape remained the same late in the failing scan:
  - timer-side `0x7/0x8/0x0`
  - consumer-side `0x17`
  - `WIFI_MAC` ISR count still advanced (`35`)

Conclusion:
- skipping the current esp32-only `set_mac_time_update_cb()` hook before `esp_wifi_init_internal()` does not restore early RX visibility
- this closes the “pre-init MAC time update callback is the primary cause” branch
- the remaining live top-level delta is now earlier than controller sequencing and narrower than this esp32-specific callback

## 2026-03-09: source comparison tightened the remaining boundary to the scheduler/runtime contract generation

- Compared the top-level init contract between:
  - working legacy `esp-wifi 0.15.1`
  - failing current `esp-radio 0.17.0`
- Legacy `esp-wifi::init(...)` explicitly performs:
  - `phy_mem_init()`
  - `setup_radio_isr()`
  - `preempt::enable()`
  - `init_tasks()`
  - `yield_task()`
  - `init_radio_clocks()`
  - `coex_initialize()`
- Current `esp_radio::init()` performs:
  - `enable_wifi_power_domain()`
  - optional legacy `phy_mem_init()` diag
  - `setup_radio_isr()`
  - `wifi_set_log_verbose()`
  - `init_radio_clocks()`
  - `coex_initialize()`
  - but only requires `preempt::initialized()` from `esp_radio_rtos_driver`
- The remaining structural difference is real in the scheduler traits too:
  - legacy external preempt integration exposes `enable()` / `disable()`
  - current `esp_radio_rtos_driver::Scheduler` exposes `initialized()` and runtime primitives only, with no equivalent bootstrap hook
- `setup_radio_isr()` on ESP32 is effectively identical between the two stacks
- `enable_wifi_power_domain()` on ESP32 is also effectively identical

Conclusion:
- the runtime-clone path is largely exhausted:
  - explicit yields
  - timer-task precreation
  - legacy-style wait loops
  - legacy-style timer loop
  - legacy `phy_mem_init()`
  - skipping `set_mac_time_update_cb()`
  have all failed as sufficient fixes
- the strongest remaining boundary is now the scheduler/runtime contract generation itself (`esp-wifi 0.15.1` preempt contract vs `esp-radio` + `esp_radio_rtos_driver`)
- this supports switching effort from more local runtime-clone A/Bs to the backend migration path

## 2026-03-09: removed two more `esp-radio` leaks from the main firmware backend seam

- Kept the main firmware build-clean while moving two backend-specific details behind `src/firmware/storage/upload/wifi/backend.rs`:
  - runtime config construction
  - `NoMem` Wi-Fi error classification
- Added backend-owned helpers:
  - `wifi_runtime_config(country_us_override: bool)`
  - `wifi_error_is_no_mem(&WifiError)`
- Replaced the higher-level direct uses of:
  - `esp_radio::wifi::CountryInfo`
  - `InternalWifiError::NoMem`
- `src/firmware/storage/upload/wifi.rs` now keeps a local no-arg `wifi_runtime_config()` wrapper only to pass the existing country-override knob into the backend-owned builder.
- `src/firmware/storage/upload/wifi/helpers.rs` now uses backend-owned `wifi_error_is_no_mem(...)` instead of matching `InternalWifiError` directly.

Validation:
- `cargo check` passes.

Why this matters:
- the main firmware Wi-Fi path now depends on fewer `esp-radio`-specific config and error internals
- this shrinks the remaining surface that a future legacy-style backend must reproduce

## 2026-03-09: moved raw runtime init/reinit plumbing out of `upload/mod.rs`

- Finished the next backend-migration refactor by moving raw runtime init and reinit sequencing into:
  - `src/firmware/storage/upload/wifi/runtime_init.rs`
- Wired that helper through:
  - `src/firmware/storage/upload/wifi.rs`
  - `src/firmware/storage/upload/mod.rs`
- `UploadHttpRuntime::setup()` no longer owns:
  - `StaticCell<RadioController>`
  - direct `init_radio()`
  - direct `new_runtime(...)`
  - direct setup-reinit sequencing
- `setup()` now only asks the Wi-Fi layer for:
  - `(WifiController<'static>, WifiDevice<'static>)`
  - then builds `embassy_net`
- Removed the now-dead setup helper copies from `upload/mod.rs`.

Validation:
- `cargo check` passes.
- LOC after the refactor:
  - `src/firmware/storage/upload/mod.rs`: `178`
  - `src/firmware/storage/upload/wifi.rs`: `295`
  - `src/firmware/storage/upload/wifi/backend.rs`: `140`
  - `src/firmware/storage/upload/wifi/runtime_init.rs`: `105`

Why this matters:
- main setup now depends on one fewer backend-specific concept
- the next legacy-backend port can replace runtime bring-up behind the Wi‑Fi seam without rewriting `UploadHttpRuntime::setup()` again

## 2026-03-09: moved setup-time storage override and early driver-state logging into the Wi‑Fi layer

- Continued the backend migration by moving setup-time Wi‑Fi driver internals out of:
  - `src/firmware/storage/upload/mod.rs`
- Added Wi‑Fi-layer helper:
  - `apply_runtime_setup_overrides_and_log()`
  - implemented in `src/firmware/storage/upload/wifi/runtime_init.rs`
- This helper now owns:
  - `esp_wifi_set_storage(WIFI_STORAGE_RAM)` override
  - early runtime driver-state telemetry (`mode`, `ps`, `protocol`, `event_mask`, `country`)
- `UploadHttpRuntime::setup()` now:
  - initializes the runtime/controller through the Wi‑Fi layer
  - applies Wi‑Fi-specific setup overrides/logging through the Wi‑Fi layer
  - only builds `embassy_net` itself

Validation:
- `cargo check` passes.
- `src/firmware/storage/upload/mod.rs` dropped to `85` LOC.

Why this matters:
- main upload setup no longer knows raw `esp_wifi_sys` storage or driver-state details
- the future legacy backend has one less firmware entry point to emulate

## 2026-03-09: moved client-mode and scan-config builders behind the backend seam

- Added backend-owned constructors in:
  - `src/firmware/storage/upload/wifi/backend.rs`
- New backend helpers now own:
  - client STA config construction from SSID/password/auth/channel/BSSID
  - active scan config construction
  - directed active scan config construction
  - channel-targeted active scan config construction
  - passive scan config construction
  - raw broad scan config construction
  - standard B/G/BGN protocol profile
  - STA mode selection
  - power-save-none selection
- Rewired call sites in:
  - `src/firmware/storage/upload/wifi/connect/config.rs`
  - `src/firmware/storage/upload/wifi/driver.rs`
  - `src/firmware/storage/upload/wifi/connect/mod.rs`
  - `src/firmware/storage/upload/wifi/connect/prepare/prepare_start.rs`
  - `src/firmware/storage/upload/wifi/connect/boot_scan_diag.rs`

Validation:
- `cargo check` passes cleanly.
- LOC after the seam cut:
  - `src/firmware/storage/upload/mod.rs`: `85`
  - `src/firmware/storage/upload/wifi.rs`: `295`
  - `src/firmware/storage/upload/wifi/backend.rs`: `232`
  - `src/firmware/storage/upload/wifi/runtime_init.rs`: `202`
  - `src/firmware/storage/upload/wifi/connect/config.rs`: `44`
  - `src/firmware/storage/upload/wifi/driver.rs`: `88`

Why this matters:
- the upload Wi‑Fi state machine now depends on fewer backend-native builders and enum constants
- the next legacy backend step can focus on implementing backend constructors instead of reproducing `esp-radio` builder APIs across the tree

## 2026-03-09: removed the dead optional legacy-backend feature stub

- Removed the unused Cargo feature stub:
  - `wifi-backend-esp-wifi-legacy`
- Removed the matching unreachable `compile_error!()` branch from:
  - `src/firmware/storage/upload/wifi/backend.rs`
- The migration direction is now explicit:
  - keep the current `wifi-backend-esp-radio`
  - port legacy behavior source-level behind the existing backend seam
  - do not model the legacy path as a dormant optional main-crate dependency

Validation:
- `cargo check` passes cleanly after removing the stub.

Why this matters:
- Cargo, code, and docs now match the actual migration strategy
- the next work item is the real legacy runtime/backend port, not a dead feature flag
