# Upload Throughput History Part 29

## 2026-03-13 - Literal Legacy Wi-Fi OS-Adapter Backend Slice

- Replaced `wifi/legacy_osi_backend.rs` delegation away from the compat shim `compat/common_adapter_legacy_osi_backend` and onto a dedicated literal backend module:
  - `vendor/esp-radio-0.17.0/src/wifi/legacy_literal_backend.rs`
- The new module is sourced from the already-ported `internal_legacy_common_backend` surface, so the internal Wi-Fi OSI table now uses one coherent literal legacy backend for:
  - semaphores
  - queues
  - mutexes
  - task creation/current task/delay
  - event/time/random/malloc helpers
  - static Wi-Fi queue helpers
- Updated:
  - `vendor/esp-radio-0.17.0/src/wifi/mod.rs`
  - `vendor/esp-radio-0.17.0/src/wifi/legacy_osi_backend.rs`
- Validation:
  - firmware toolchain build passes via `CARGO_FEATURES=wifi-debug-slim-app scripts/build/build.sh debug`
- Interpretation:
  - this is a wholesale migration step, not an A/B hook tweak
  - next step is canonical `backend_legacy_port` full-flash validation from this new literal backend baseline

## 2026-03-13 - Literal legacy wifi_start/scan_n control path closes without moving delivery

- Switched the `backend_legacy_port` control path to the imported literal legacy
  `wifi_start()` / blocking broad `scan_n(max)` shape:
  - `vendor/esp-radio-0.17.0/src/wifi/internal_legacy_control_backend.rs`
  - `vendor/esp-radio-0.17.0/src/wifi/internal_legacy_scan_literal.rs`
- Validation:
  - canonical slim-app full-flash `hostctl flash-capture`
  - artifact:
    - `logs/hostctl_flashcapture_backend_legacy_port_20260313_literal_control_scan_fixed2/capture.log`
- Result:
  - `backend_legacy_port` still fully initializes and reaches `start=ok`
  - pre-scan promisc remains zero on `8/1/6/11`
  - `wifi_mac_isr_count` rises
  - `wifi_rx_cb_count sta=0 ap=0`
  - raw `ScanDone` list remains empty
  - wrapped scan still fails quickly with:
    - `scan_err elapsed_ms=9 err=InternalError(Timeout)`
  - direct null scan still fails with:
    - `scan_start_err scan_rc=12300`
  - direct explicit scan still runs and returns:
    - `ap_num=0`
  - scan-time runtime counters remain flat:
    - `queue_send=0`
    - `queue_send_isr=0`
    - `queue_recv=0`
    - `sem_take=0`
    - `sem_give=0`
    - `thread_sem_get=0`
- Interpretation:
  - the old Rust-side control/wrapper shape is no longer the missing slice
  - the remaining gap is deeper than scan-config/control construction and now
    appears to sit below the Rust-side internal control layer, in the older vs
    newer internal Wi-Fi/blob-facing behavior itself

## 2026-03-16 - Literal legacy admission module closes without moving scan-time delivery

- Collapsed the `backend_legacy_port` start/stop/scan path into one literal
  legacy admission module:
  - `vendor/esp-radio-0.17.0/src/wifi/internal_legacy_admission_literal.rs`
- Re-routed the active backend path to that module:
  - `vendor/esp-radio-0.17.0/src/wifi/internal_legacy_backend.rs`
  - `vendor/esp-radio-0.17.0/src/wifi/mod.rs`
- The new module now owns, in one place:
  - `esp_wifi_start()` plus inactive-time setup
  - blocking `esp_wifi_stop()`
  - `scan_with_config()`
  - legacy broad `scan_n(max)`
  - zeroed `wifi_scan_config_t` construction
  - AP result retrieval/clear
- Validation:
  - firmware build:
    - `CARGO_FEATURES=wifi-debug-slim-app scripts/build/build.sh debug`
  - canonical full-flash hostctl boot-scan capture with:
    - `MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG=1`
    - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
    - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST=1`
    - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE=1`
    - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_PROMISC_DIAG=1`
  - artifact:
    - `logs/hostctl_flashcapture_backend_legacy_port_20260316_075226_literal_admission_bootscan/capture.log`
- Result:
  - `backend_legacy_port` is active:
    - `wifi_backend name=backend-legacy-port`
  - init still completes fully:
    - `legacy_port runtime_init result=ok`
    - `legacy_port_wifi_init stage=done`
    - `boot_scan_only_diag start=ok`
  - but the scan boundary does not move:
    - pre-scan promisc still zero on `8/1/6/11`
    - direct null scan still fails:
      - `idf_compare=scan_start_err scan_rc=12300`
    - direct explicit scan still returns zero:
      - `idf_explicit_compare=ok ... ap_num=0`
    - wrapped scan still fails early:
      - `outcome=scan_err elapsed_ms=9 err=InternalError(Timeout)`
    - `wifi_mac_isr_count` rises:
      - `after=idf_compare_first count=13`
      - `after=idf_explicit_compare_first count=41`
      - `after=rust_scan_err count=68`
    - `wifi_rx_cb_count` stays dark:
      - `sta=0 ap=0`
    - raw `ScanDone` remains empty
  - scan-time runtime counters remain flat:
    - `queue_send=0`
    - `queue_send_isr=0`
    - `queue_recv=0`
    - `sem_take=0`
    - `sem_give=0`
    - `thread_sem_get=0`
    - `legacy_task_model entry_count=0`
  - while the persistent legacy runtime remains active:
    - `legacy_builtin_scheduler initialized=1`
    - `legacy_preempt_builtin initialized=1`
- Interpretation:
  - collapsing start/stop/scan into one literal legacy admission module is
    structurally correct, but it still does not restore packet delivery or scan
    admission
  - the remaining gap is now clearly below the Rust-side internal admission
    surface and inside the deeper old-vs-new internal Wi-Fi/blob behavior
