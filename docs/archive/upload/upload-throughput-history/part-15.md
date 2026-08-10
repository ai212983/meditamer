# Upload Throughput History Part 15

## 2026-03-06: rejected C-like discovery-start ordering as blackout root cause

Objective:

- test whether the Rust blackout is caused by applying full STA config before the
  first `start_async()` / scan sequence.

Variant:

- enabled `MEDITAMER_WIFI_C_LIKE_DISCOVERY_START=1`
- kept first-start blackout telemetry enabled:
  - `MEDITAMER_WIFI_EARLY_DRIVER_STATE_DIAG=1`
  - `MEDITAMER_WIFI_POST_START_PROMISC_DIAG=1`
  - `MEDITAMER_WIFI_START_RAW_SCAN_DIAG=1`
  - `MEDITAMER_WIFI_FIRST_START_IDF_LOG_DIAG=1`

Artifacts:

- successful C control comparison run:
  - `logs/esp_idf_wifi_control_scan_20260306_100150.log`
- Rust boot log for this variant:
  - `logs/boot_runtime_diag_clike_20260306_100756/boot_espflash_monitor.log`
- Rust first-start raw transcript:
  - `logs/boot_runtime_diag_clike_20260306_100756/start_from_waiting.raw.log`

Key findings:

- the new ordering took effect exactly as intended:
  - `c_like_discovery_start enabled; starting bare STA before first scan`
  - pre-start STA config stayed empty:
    - `pre_start_sta_config ... ssid_len=0 ... threshold_authmode=0`
  - first-start STA config also stayed empty until after the raw scan:
    - `first_start_sta_config ... ssid_len=0 ... threshold_authmode=0`
- despite that, the blackout shape was unchanged:
  - immediate post-start promiscuous RX remained zero:
    - `post_start_promisc_diag ... total=0`
  - immediate raw broad scan remained zero before config application:
    - `start_raw_scan_diag ... result_count=0`
  - only after that did firmware apply the station config:
    - `applying station config auth=WpaWpa2Personal ...`
  - the following discovery scan was still all-zero:
    - `scan_stage end label=active_broad ... result_count=0`

Conclusion:

- pre-start `set_config(...)` ordering is not the primary cause of discovery
  blackout in this Rust path.
- this result is stronger than earlier config hypotheses because the radio was
  already dark in a bare-STA start with no SSID/auth/BSSID/channel preloaded.
- the remaining root-cause target stays lower in the Rust `esp-radio` startup
  path or its driver integration, not in upload-task discovery policy.

Next step:

- build the smallest possible Rust-side scan-only repro on top of the current
  `esp-radio` path and compare its init/start ordering directly against the
  successful C/ESP-IDF control app.

## 2026-03-06: minimal Rust boot scan-only repro still blackouts

Objective:

- remove the upload connection state machine entirely from the first-start
  experiment and test the smallest Rust-side `esp-radio` path:
  `set_mode(STA) -> start -> broad scan -> stop`.

Variant:

- enabled `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
- the diagnostic runs once at boot, before `waiting for NETCFG credentials`.

Artifact:

- `logs/boot_scan_only_diag_20260306_091740/boot_espflash_monitor.log`
- `logs/boot_scan_only_diag_idfcompare_20260306_092204/boot_espflash_monitor.log`

Observed:

- the Rust boot-only scan path executed cleanly:
  - `boot_scan_only_diag begin credentials_present=false`
  - `event sta_start`
  - `boot_scan_only_diag start=ok`
  - `event scan_done status=0 count=0 scan_id=128`
  - `boot_scan_only_diag scan=ok elapsed_ms=176 result_count=0`
  - `event sta_stop`
  - `boot_scan_only_diag stop=ok`
- immediately after that, firmware returned to the usual idle state:
  - `waiting for NETCFG credentials over UART`
- the direct IDF comparator inside the same minimal Rust boot repro was also
  zero:
  - `boot_scan_only_diag idf_compare=ok ... ap_num=0 records_returned=0`

Conclusion:

- the blackout reproduces even on the smallest Rust-side `esp-radio`
  scan-only control path, before the upload connection task, `NETCFG`, scan
  ladder, or recovery logic can matter.
- because the direct IDF scan is also zero after `esp-radio` initialization in
  that minimal path, the primary fault is not just the Rust scan wrapper or
  event-future layer.
- combined with the successful official C/ESP-IDF scan on the same board and
  network, this narrows the problem further to Rust `esp-radio` bring-up /
  integration rather than the higher-level upload Wi-Fi state machine.

Next step:

- inspect `esp-radio::wifi_init()` / `wifi::new(...)` and patch the remaining
  init-path deltas against C, prioritizing the ones that can affect RX before
  scan:
  - `esp_wifi_init_internal(...)` vs public `esp_wifi_init(...)`
  - forced `WIFI_MODE_NULL`
  - `esp_supplicant_init()`
  - custom `esp_wifi_internal_reg_rxcb(...)`

## 2026-03-06: local esp-radio init-path single-delta A/Bs did not restore scans

Objective:

- test the remaining single-delta init-path differences inside local
  `esp-radio 0.17.0` without changing the firmware logic above it.

Method:

- patched the local Cargo registry copy of `esp-radio` to allow compile-time
  skipping of individual init steps:
  - internal RX callback registration
  - `esp_supplicant_init()`
  - forced initial `WIFI_MODE_NULL`
- reran the same boot-only Rust repro each time:
  - `set_mode(STA) -> start -> esp-radio scan -> direct IDF scan -> stop`

Artifacts:

- skip internal RX callback registration:
  - `logs/boot_scan_only_diag_skiprxcb_20260306_092929/boot_espflash_monitor.log`
- skip supplicant init:
  - `logs/boot_scan_only_diag_skipsupp_20260306_093024/boot_espflash_monitor.log`
- skip initial `WIFI_MODE_NULL`:
  - `logs/boot_scan_only_diag_skipnull_20260306_093115/boot_espflash_monitor.log`

Observed:

- all three variants still produced the same shape:
  - `boot_scan_only_diag scan=ok ... result_count=0`
  - `boot_scan_only_diag idf_compare=ok ... ap_num=0 records_returned=0`
- none of the three single-delta A/Bs restored AP visibility.
- the fourth init-entrypoint A/B also had no effect:
  - `MEDITAMER_WIFI_ESP_RADIO_USE_PUBLIC_INIT=1`
  - `logs/boot_scan_only_diag_publicinit_20260306_093405/boot_espflash_monitor.log`
  - still `boot_scan_only_diag scan=ok ... result_count=0`
  - still `boot_scan_only_diag idf_compare=ok ... ap_num=0 records_returned=0`

Conclusion:

- those three init-path side effects are not individually sufficient to explain
  the blackout.
- switching the initializer entrypoint from `esp_wifi_init_internal(...)` to the
  public `esp_wifi_init(...)` is also not sufficient to restore RX / scan
  visibility.

## 2026-03-06: direct C-lifecycle preinit is blocked in this no-`esp-idf` Rust link

Objective:

- test the next major lifecycle delta against the working C control app by
  running `esp_netif_init() -> esp_event_loop_create_default() ->
  esp_netif_create_default_wifi_sta()` before the same Rust boot scan-only
  repro.

Attempt:

- verified the symbols are present in the ESP-IDF/C control world:
  - `esp_event_loop_create_default`
  - `esp_event_loop_delete_default`
  - `esp_netif_create_default_wifi_sta`
  - `esp_netif_init` via `esp-idf-svc`
- tried a narrow Rust firmware build that linked `esp_event` and `esp_netif`
  archives from the external C control build output.

Observed:

- the current no-`esp-idf` Rust firmware link does not include the wider ESP-IDF
  component set that those calls pull in.
- once `esp_event` / `esp_netif` were added, the link cascaded into broader
  unresolved ESP-IDF / FreeRTOS / ROM symbol requirements rather than producing
  a runnable firmware image.
- this means the direct C-lifecycle preinit experiment is presently blocked by
  link model mismatch, not by missing headers or call-site access.

Conclusion:

- the next C lifecycle delta is not practically testable inside the current
  firmware image without importing a much larger ESP-IDF component/link surface.
- that raises the confidence that the boundary is architectural:
  the working C app runs on an ESP-IDF netif/event-loop lifecycle, while the
  current Rust firmware runs on `esp-radio` without that stack.

Next step:

- stop trying to inject ESP-IDF netif/event-loop pieces into the current image
  ad hoc.
- instead, compare `esp-radio` startup behavior against upstream issue traffic
  and source history, or build a separate Rust-on-ESP-IDF control probe if we
  want a like-for-like netif/event-loop comparator.

## 2026-03-06: separate Rust-on-ESP-IDF control probe scaffolded; current blocker is IDF version compatibility

Objective:

- build a like-for-like Rust control probe on top of `esp-idf-svc` so the
  comparison is `Rust + ESP-IDF lifecycle` versus `Rust + esp-radio`, rather
  than `C + ESP-IDF lifecycle` versus `Rust + esp-radio`.

Implementation:

- added standalone tool crate:
  - `tools/esp_idf_wifi_control_rust/Cargo.toml`
  - `tools/esp_idf_wifi_control_rust/build.rs`
  - `tools/esp_idf_wifi_control_rust/src/main.rs`
- added wrapper:
  - `scripts/device/wifi_control_idf_rust.sh`
- the probe is scan-only:
  - `EspWifi::new(...) -> BlockingWifi::wrap(...) -> start() -> scan() -> stop()`

Observed:

- the first build blocker was missing `std` for `xtensa-esp32-espidf`; fixed by
  switching to `cargo +esp` plus `-Zbuild-std=std,panic_abort` and
  `--cfg espidf_time64`
- the next blocker is version compatibility:
  - external ESP-IDF `v5.5.2` fails the Rust probe build in `esp-idf-hal 0.45.2`
    on TWAI bindings (`twai_timing_config_t` layout mismatch)
  - local crate changelog explicitly claims compatibility through ESP-IDF 5.3.x,
    not 5.5.x
- started installing external ESP-IDF `v5.3.4` under the parent `.esp-idf`
  directory to get onto a supported branch, but did not finish the full install
  within this run

Conclusion:

- the separate Rust-on-ESP-IDF comparison path is now scaffolded and
  reproducible
- the remaining blocker is not probe design; it is finishing a supported
  external ESP-IDF 5.3.x install and rerunning the build/flash against that
  version

Next step:
- complete the external `v5.3.4` install, build/flash the Rust-on-ESP-IDF
  probe, and compare its scan output against:
  - `logs/esp_idf_wifi_control_scan_20260306_100150.log`
  - `logs/boot_scan_only_diag_idfcompare_20260306_092204/boot_espflash_monitor.log`
## 2026-03-06: Rust-on-ESP-IDF control probe scans successfully; blackout boundary narrows to current `esp-radio` integration
Objective:
- finish the like-for-like Rust comparison on top of ESP-IDF and determine
  whether the blackout is specific to `esp-radio` / bare-metal startup rather
  than to Rust itself.
Implementation:
- completed the external ESP-IDF `v5.3.4` install under the parent `.esp-idf`
  directory
- fixed the Rust probe toolchain path by cleaning stale generated build state
  after the earlier `v5.5.2` attempt and adding
  `tools/esp_idf_wifi_control_rust/.cargo/config.toml` with
  `linker = "ldproxy"`
- rebuilt and flashed the probe via `scripts/device/wifi_control_idf_rust.sh`
Observed:
- the Rust-on-ESP-IDF probe now boots and scans successfully on the same board
  and network
- bounded monitor capture: `logs/esp_idf_wifi_control_rust_scan_20260306_111949.log`
- key lines from that log:
  - `esp_idf_wifi_control_rust: mode=scan_only started=true`
  - `esp_idf_wifi_control_rust: scan_complete total_ap_count=9`
  - 9 APs listed, including `<nearby-ssid-1>`, `<test-ssid-primary>`,
    `<test-ssid-guest>`, `<nearby-ssid-2>`, and `<nearby-ssid-3>`
- this matches the successful C control result at
  `logs/esp_idf_wifi_control_scan_20260306_100150.log`
- and differs from the failing `esp-radio` minimal repro at
  `logs/boot_scan_only_diag_idfcompare_20260306_092204/boot_espflash_monitor.log`,
  where both the Rust scan wrapper and direct IDF scan remained zero

Conclusion:

- the blackout is not caused by Rust in general
- the blackout is not caused by the board, RF environment, or ESP-IDF Wi-Fi
  stack itself
- the current fault boundary remains `esp-radio` / no-`esp-idf`
  initialization or integration path even after restoring the published
  `esp-radio 0.17.0` baseline

Next step:

- stop spending time on generic RF/persistence hypotheses
- inspect and instrument the current `esp-radio` initialization path against
  the successful Rust-on-ESP-IDF and C/ESP-IDF lifecycles, then decide between
  a local `esp-radio` vendor patch and an upstream esp-rs issue with the
  minimal repro evidence
