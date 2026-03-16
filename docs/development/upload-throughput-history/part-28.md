# Upload Throughput History Part 28

## 2026-03-11 - Dedicated Legacy Common-Adapter Backend Split

- added a dedicated legacy blob-facing callback module:
  - `vendor/esp-radio-0.17.0/src/compat/common_adapter_legacy_backend.rs`
- rewired the mixed modern common adapter to dispatch `backend_legacy_port`
  through that module for legacy semaphore/queue callbacks
- left the modern callback path intact for non-legacy runtime modes

Why this step exists:
- it mirrors the earlier dedicated legacy Wi-Fi os-adapter split
- it removes another mixed modern/legacy layer from the wholesale
  `backend_legacy_port` path
- it is aligned with the vendoring plan in
  `docs/development/wifi-legacy-vendoring-plan.md`

Build result:
- `cargo check` passes after the split

Runtime status:
- this is structural migration progress
- the last canonical validation boundary still remains inside
  `esp_wifi_init_internal(...)`
- the next validation should be run through canonical hostctl flash-capture,
  not standalone hook experiments

## 2026-03-11: Dedicated legacy preempt adapter did not move the esp_wifi_init_internal stall

We added a dedicated vendored legacy preempt adapter for `backend_legacy_port`:

- `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/vendor/esp-radio-0.17.0/src/compat/preempt_legacy_backend.rs`
- `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/vendor/esp-radio-0.17.0/src/compat/common_legacy.rs`
- `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/vendor/esp-radio-0.17.0/src/compat/mod.rs`

This moved the legacy path off the mixed `preempt_backend` selector for:

- `current_task`
- `current_task_thread_semaphore`
- `task_create`
- `schedule_task_deletion`
- `yield_task`

Canonical validation used the repo-prescribed `flash.sh` + hostctl `flash-capture` flow.

Result:

- boot still reaches `legacy_port_wifi_init stage=esp_wifi_init_internal.before`
- then reaches:
  - `semphr_create.after`
  - `semphr_take`
  - `semphr_give`
  - `wifi_thread_semphr_get.after`
- and then makes no further visible progress inside `esp_wifi_init_internal(...)`

Artifact:

- `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/logs/flash_capture_20260311_175340/capture.log`

Conclusion:

- the dedicated legacy preempt adapter is structurally correct
- but it does not move the wholesale-port boundary
- the next real slice is a more literal source import/replacement of the legacy `preempt_builtin` runtime itself, not another adapter-layer reroute

## 2026-03-11: Dedicated legacy common-adapter path did not move the init boundary

We expanded the wholesale legacy port by routing more of the blob-facing common-adapter
surface through the dedicated legacy backend module instead of the mixed modern
`common_adapter.rs` bodies.

Ported slice:

- `vendor/esp-radio-0.17.0/src/compat/common_adapter_legacy_backend.rs`
  - legacy queue create/delete
  - legacy semaphore/queue callbacks
  - legacy `esp_timer_get_time`
  - legacy `vTaskDelay`
- `vendor/esp-radio-0.17.0/src/common_adapter.rs`
  - dispatches `backend_legacy_port` through that module for the ported common-adapter
    surface

Canonical validation used the slim app and the standard hostctl flash-capture path.

Artifact:

- `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/logs/flash_capture_20260311_212032/capture.log`

Result:

- runtime still reaches:
  - `legacy_port runtime_init stage=before_wifi_new`
  - `legacy_port wifi_new stage=begin`
  - `legacy_port_wifi_new stage=wifi_init`
  - `legacy_port_wifi_init stage=esp_wifi_init_internal.before`
- and then stops inside `esp_wifi_init_internal(...)`

Conclusion:

- the dedicated legacy common-adapter port is structurally correct
- but it does not move the wholesale-port boundary
- the next wholesale slice must continue below adapter dispatch and deeper into the
  literal legacy runtime/init substrate expected by `esp_wifi_init_internal(...)`
## 2026-03-12: Direct `common_legacy` routing did not move the legacy-port init boundary

- Replaced the dedicated legacy common-adapter forwarding layer to call the
  imported legacy `compat/common_legacy.rs` bodies directly instead of the
  adapted `common_legacy_backend` shim.
- Files:
  - `vendor/esp-radio-0.17.0/src/compat/common_adapter_legacy_backend.rs`
- Validation:
  - canonical slim-app `backend_legacy_port` hostctl flash-capture
  - artifacts:
    - `logs/hostctl_flashcapture_backend_legacy_port_20260312_074602_common_legacy_direct/capture.log`
    - `logs/hostctl_flashcapture_backend_legacy_port_20260312_074602_common_legacy_direct/summary.txt`
- Result:
  - runtime still reaches:
    - `legacy_port runtime_init stage=before_wifi_new`
    - `legacy_port wifi_new stage=begin`
    - `legacy_port_wifi_new stage=wifi_init`
    - `legacy_port_wifi_init stage=esp_wifi_init_internal.before`
  - and then still blocks inside `esp_wifi_init_internal(...)`
- Interpretation:
  - the dedicated legacy common-adapter surface is no longer the likely missing
    substrate for `backend_legacy_port`
  - the next wholesale slice should move inward to the literal internal Wi-Fi
    backend/init substrate rather than more common-adapter reroutes

## 2026-03-12: Literal legacy `scan_n()` broad-scan slice did not restore RX admission

- Replaced the `backend_legacy_port` broad active scan path with the working
  legacy blocking `scan_n()` shape for unfiltered active scans.
- Files:
  - `vendor/esp-radio-0.17.0/src/wifi/mod.rs`
- Validation:
  - canonical slim-app full-flash `hostctl flash-capture`
  - artifacts:
    - `logs/hostctl_flashcapture_backend_legacy_port_20260312_130830_legacy_scan_n_fullflash/flash_capture.log`
    - `logs/flash_capture_20260312_130831/capture.log`
- Result:
  - init still completes fully and `boot_scan_only_diag start=ok`
  - pre-scan promisc remains zero on `8/1/6/11`
  - wrapped legacy-port scan still fails early with:
    - `boot_scan_only_diag outcome=scan_err elapsed_ms=17 err=InternalError(Timeout)`
  - direct IDF explicit scan still completes with:
    - `status=1 count=0`
    - empty raw `ScanDone` list
    - `wifi_rx_cb_count sta=0 ap=0`
- Interpretation:
  - the wholesale port is now using the simpler legacy blocking broad-scan
    shape, so the surviving boundary is below scan wrapper shape and still
    above packet delivery / scan admission in the current blob/runtime
    generation

## 2026-03-12: Dedicated legacy RX delivery backend extracted for `backend_legacy_port`

- Added a dedicated legacy RX delivery module:
  - `vendor/esp-radio-0.17.0/src/wifi/legacy_rx_backend.rs`
- Switched the legacy delivery shim to delegate queue storage and sniffer
  callback storage to that module:
  - `vendor/esp-radio-0.17.0/src/wifi/legacy_delivery.rs`
- Registered the new module in:
  - `vendor/esp-radio-0.17.0/src/wifi/mod.rs`

Why this slice:
- The vendoring plan requires moving literal legacy packet-delivery behavior
  into dedicated backend modules instead of extending mixed modern bodies.
- Legacy RX callback enqueue and legacy sniffer callback storage were already
  logically selected for `backend_legacy_port`, but they still lived inside a
  transitional shim. This split makes RX delivery a first-class wholesale port
  surface.

Validation:
- `CARGO_FEATURES=wifi-debug-slim-app scripts/build/build.sh debug`
  completed successfully after the split.

Interpretation:
- This is a structural migration step, not yet a behavioral result.
- The next step is canonical `backend_legacy_port` validation through the full
  flash/capture path.

## 2026-03-12: Core legacy Wi-Fi os-adapter routing did not activate persistent legacy runtime participation

- Re-routed the core task/mutex/thread-semaphore path in the dedicated legacy
  Wi-Fi os-adapter backend so `backend_legacy_port` now calls the literal
  legacy-port helpers directly rather than the mixed backend shim.
- Files:
  - `vendor/esp-radio-0.17.0/src/wifi/os_adapter/legacy_backend.rs`
- Validation:
  - canonical slim-app full-flash `hostctl flash-capture`
  - artifact:
    - `logs/flash_capture_20260312_162924/capture.log`
- Result:
  - runtime still completes fully and reaches `boot_scan_only_diag start=ok`
  - pre-scan promisc is still zero on `8/1/6/11`
  - wrapped scan still fails early with:
    - `outcome=scan_err elapsed_ms=14 err=InternalError(Timeout)`
  - direct null scan still fails with:
    - `idf_compare=scan_start_err scan_rc=12300`
  - direct explicit scan still completes with:
    - `idf_explicit_compare=ok ... ap_num=0`
  - raw `ScanDone` list remains empty
  - `wifi_mac_isr_count` rises, but:
    - `wifi_rx_cb_count sta=0 ap=0`
    - `queue_send=0`
    - `queue_send_isr=0`
    - `queue_recv=0`
    - `sem_take=0`
    - `sem_give=0`
    - `thread_sem_get=0`
    - `legacy_task_model entry_count=0`
    - `legacy_builtin_scheduler initialized=0`
    - `legacy_preempt_builtin initialized=1`
- Interpretation:
  - the ported legacy init/bootstrap path is active, but the persistent
    scan-time runtime is still not participating
  - the next wholesale slice must make `backend_legacy_port` use the legacy
    preempt/scheduler runtime as the persistent runtime after init, not just as
    a bootstrap

## 2026-03-13: Dedicated internal legacy packet backend did not restore packet delivery

- Switched `legacy_port_wifi_init()` RX callback registration off the older
  `legacy_packet_delivery` path and onto the dedicated internal packet backend:
  - `vendor/esp-radio-0.17.0/src/wifi/mod.rs`
  - `vendor/esp-radio-0.17.0/src/wifi/internal_legacy_packet_backend.rs`
- Validation:
  - canonical slim-app full-flash `hostctl flash-capture`
  - artifact:
    - `logs/hostctl_flashcapture_backend_legacy_port_20260313_100539_internal_legacy_packet/capture.log`
- Result:
  - `backend_legacy_port` still completes:
    - `legacy_port runtime_init result=ok`
    - `legacy_port_wifi_init stage=...done`
    - `boot_scan_only_diag start=ok`
  - pre-scan promisc remains zero on `8/1/6/11`
  - `wifi_mac_isr_count` rises, but:
    - `wifi_rx_cb_count sta=0 ap=0`
  - wrapped scan still fails early with:
    - `outcome=scan_err elapsed_ms=11 err=InternalError(Timeout)`
  - direct null scan still fails with:
    - `idf_compare=scan_start_err scan_rc=12300`
  - direct explicit scan still completes with:
    - `idf_explicit_compare=ok ... ap_num=0`
  - raw `ScanDone` list remains empty
  - scan-time runtime counters remain flat:
    - `queue_send=0`
    - `queue_send_isr=0`
    - `queue_recv=0`
    - `sem_take=0`
    - `sem_give=0`
    - `thread_sem_get=0`
    - `legacy_task_model entry_count=0`
  - while:
    - `legacy_builtin_scheduler initialized=1`
    - `legacy_preempt_builtin initialized=1`
- Interpretation:
  - switching to the dedicated internal legacy packet backend is structurally
    correct, but it does not move the packet-delivery boundary
  - the remaining gap is below runtime/bootstrap activation and below wrapper
    scan shape, in the internal Wi-Fi delivery/admission layer itself
