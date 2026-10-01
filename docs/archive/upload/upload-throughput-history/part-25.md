# Upload Throughput History, Part 25

## 2026-03-09: created `backend_legacy_port` staging module for the source-level migration

- Added a compile-clean staging module tree:
  - `src/firmware/storage/upload/wifi/backend_legacy_port/mod.rs`
  - `src/firmware/storage/upload/wifi/backend_legacy_port/bootstrap.rs`
  - `src/firmware/storage/upload/wifi/backend_legacy_port/contracts.rs`
  - `src/firmware/storage/upload/wifi/backend_legacy_port/availability.rs`
- Wired the module into:
  - `src/firmware/storage/upload/wifi.rs`
- The staging module does not switch runtime selection yet.
- It captures the proven working legacy port contract in code:
  - expected bootstrap sequence
  - scheduler bootstrap requirements
  - effective init-config invariants
  - Wi‑Fi task contract
  - scope of the first real port increment (`init/start/scan/stop` first, connect/device path deferred)

Validation:
- `cargo check` passes cleanly.
- LOC:
  - `backend_legacy_port/mod.rs`: `20`
  - `backend_legacy_port/bootstrap.rs`: `37`
  - `backend_legacy_port/contracts.rs`: `57`
  - `backend_legacy_port/availability.rs`: `57`

Why this matters:
- the legacy backend port now has an in-tree source target instead of only history notes and standalone tools
- the next step is to replace staging constants with real ported bootstrap/runtime code, not to invent the contract again

## 2026-03-09: encoded the first real blocker inside `backend_legacy_port`

- Added:
  - `src/firmware/storage/upload/wifi/backend_legacy_port/availability.rs`
- This now records which proven legacy bootstrap hooks are actually reachable from the current stack:
  - available:
    - `enable_wifi_power_domain`
    - `phy_mem_init`
    - `setup_radio_isr`
    - `wifi_set_log_verbose`
    - `init_radio_clocks`
    - `coex_initialize`
  - missing:
    - `preempt::enable`
    - `init_tasks`
    - explicit legacy `initial_yield`

Validation:
- `cargo check` passes cleanly.

Why this matters:
- the migration blocker is now explicit in code, not just inferred from history
- the next port step must move below the firmware layer and either:
  - vendor/recreate the missing scheduler bootstrap semantics, or
  - expose equivalent hooks from the current runtime generation

## 2026-03-09: explicit legacy-preempt compatibility surface is wired, but still insufficient

- Added a deeper vendored runtime compatibility surface:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/legacy_preempt.rs`
- Re-exported it through:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/mod.rs`
  - `vendor/esp-rtos-0.2.0/src/lib.rs`
- Switched the legacy-port firmware path to use that explicit compatibility status:
  - `src/firmware/storage/upload/wifi/backend_legacy_port/bootstrap.rs`
  - `src/firmware/storage/upload/wifi/backend_legacy_port/runtime.rs`
- Validated the unified firmware path with:
  - `MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG=1`
  - boot-scan comparators and pre-scan promisc enabled

Validation:
- Flash log:
  - `logs/boot_scan_backend_legacy_port_preemptcompat_20260309_173535/flash.log`
- Boot log:
  - `logs/boot_scan_backend_legacy_port_preemptcompat_20260309_173535/monitor.log`
- Key runtime lines:
  - `legacy_port_runtime name=backend-legacy-port ... requires_enable=true requires_task_bootstrap=true requires_initial_yield=true`
  - `legacy_port_bootstrap scheduler_initialized=true current_core_initialized=true timer_task_precreated=true timer_task_started=true yielded_once=true`
  - `legacy_port runtime_init result=ok`

Observed result:
- pre-scan promisc still stayed fully dark:
  - `boot_scan_only_promisc_diag ... total=0`
- direct IDF `NULL` scan still returned zero:
  - `idf_compare=ok ... ap_num=0`
- direct IDF explicit broad scan still returned zero:
  - `idf_explicit_compare=ok ... ap_num=0`
- wrapped backend scan still returned zero:
  - `scan=ok elapsed_ms=206 result_count=0`

Why this matters:
- the deeper compatibility surface is now real and exercised, not just a bootstrap shim note
- but even the stronger legacy-preempt approximation is still insufficient
- this closes the branch "current runtime can be made legacy-equivalent with bootstrap compatibility alone"
- the next remaining work is the deeper source-level legacy runtime/backend port, not more nearby compatibility toggles

## 2026-03-09: `backend_legacy_port` now owns controller `start/scan/stop`

- Added:
  - `src/firmware/storage/upload/wifi/backend_legacy_port/controller.rs`
- Re-exported through:
  - `src/firmware/storage/upload/wifi/backend_legacy_port/mod.rs`
- Switched the generic backend glue to delegate legacy-port controller behavior into that module:
  - `src/firmware/storage/upload/wifi/backend.rs`

Validation:
- `cargo check` passes cleanly.

Why this matters:
- the legacy backend port now owns both:
  - runtime bootstrap/init selection
  - controller `start/scan/stop` semantics
- that removes another chunk of legacy behavior from generic `backend.rs` and keeps the source-level port concentrated under `backend_legacy_port`
- the next deeper port step can now focus on legacy runtime/init semantics without re-spreading controller behavior back into generic backend glue

## 2026-03-09: `backend_legacy_port` now owns its runtime config, direct init path, and local types

- Added:
  - `src/firmware/storage/upload/wifi/backend_legacy_port/config.rs`
  - `src/firmware/storage/upload/wifi/backend_legacy_port/types.rs`
- Updated:
  - `src/firmware/storage/upload/wifi/backend_legacy_port/runtime.rs`
  - `src/firmware/storage/upload/wifi/backend_legacy_port/controller.rs`
  - `src/firmware/storage/upload/wifi/backend_legacy_port/mod.rs`
  - `src/firmware/storage/upload/wifi/runtime_init.rs`

Validation:
- `cargo check` passes cleanly.

What changed:
- the legacy-port runtime path now builds its own runtime config instead of receiving generic backend config plumbing
- the legacy-port runtime path now calls `esp_radio::init()` / `esp_radio::wifi::new(...)` directly inside the port module
- the legacy-port module now owns local type aliases for:
  - `RadioController`
  - `WifiController`
  - `WifiDevice`
  - `WifiError`
  - `AccessPointInfo`
  - `ScanConfig`

Why this matters:
- this is a deeper source-level port step than the earlier seam extraction
- generic `backend.rs` is now more clearly the current backend shim, while `backend_legacy_port` owns its own direct runtime/controller surface
- the remaining blocker is no longer module ownership; it is the missing deeper legacy runtime behavior itself

## 2026-03-09: legacy-style Xtensa task entry still does not restore scanning

- Added a vendored runtime-port A/B in:
  - `vendor/esp-rtos-0.2.0/src/task/xtensa.rs`
- New knob:
  - `MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_TASK_ENTRY_DIAG=1`

What it changes:
- current `esp-rtos` can initialize `esp-radio` tasks with legacy-style direct Xtensa task entry:
  - `PC = task_fn`
  - `A6 = param`
- instead of the current trampoline form:
  - `PC = task_wrapper`
  - `A6 = task_fn`
  - `A7 = param`

Validation:
- isolated current standalone comparator:
  - `logs/esp_radio_nostd_wifi_control_legacytaskentry_validate_20260309_berlin/monitor.log`
- key lines:
  - `begin=true`
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `start=ok`
  - `scan=ok count=0`

Why this matters:
- this is a real task/preempt bring-up difference, not a wrapper-level timing knob
- even with legacy-style task entry, the current standalone `esp-radio` path still scans zero
- that closes another concrete runtime-port branch and pushes the remaining work deeper into task/preempt behavior than entry register setup alone

## 2026-03-09: legacy Wi-Fi task handoff and zero-priority model still do not restore scanning

- Added a vendored runtime-port handoff helper:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/task_bootstrap.rs`
- Wired it through:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/mod.rs`
  - `vendor/esp-rtos-0.2.0/src/scheduler.rs`
  - `vendor/esp-rtos-0.2.0/src/lib.rs`
  - `tools/esp_radio_nostd_wifi_control/src/main.rs`

New knobs:
- `MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_WIFI_TASK_BOOTSTRAP_DIAG=1`
  - after creating the `wifi` task, current `esp-rtos` yields repeatedly until the task has been selected at least once
- `MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_WIFI_TASK_PRIORITY_MODEL_DIAG=1`
  - the created `wifi` task uses priority `0`, approximating the legacy no-priority/circular scheduler model more closely than the default priority `29`

Validation:
- handoff-only run:
  - `logs/esp_radio_nostd_wifi_control_legacywifihandoff_20260309_berlin/monitor.log`
- combined handoff + zero-priority run:
  - `logs/esp_radio_nostd_wifi_control_legacypriorityhandoff_20260309_berlin/monitor.log`

Observed result:
- both runs still reached clean startup:
  - `begin=true`
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `start=ok`
- both still ended at:
  - `scan=ok count=0`

Why this matters:
- this closes the next scheduler-model approximations:
  - explicit `wifi` task handoff after create
  - legacy-like zero-priority `wifi` task creation
- the remaining scheduler gap is no longer simple task entry, priority, or first-handoff behavior
- the next real step would be a deeper run-queue / circular-scheduler model port, not another local knob

## 2026-03-09: corrected three-task legacy task model still does not restore RX visibility

- Extended the vendored legacy task-model port to register the internal `timer` task too:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/timer_queue.rs`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/legacy_scheduler.rs`

What changed:
- the `timer` task is now inserted into the legacy task-model ring at both current creation sites
- the `wifi`-task selection bug was fixed so creating `wifi` no longer hardcodes `CURRENT_INDEX=1`; it now selects the actual index of the new `wifi` task in the ring

Validation:
- isolated current standalone comparator with:
  - `MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_ESP_RADIO_TASK_MODEL_DIAG=1`
- summary artifact:
  - `logs/esp_radio_nostd_wifi_control_legacytaskmodel_timerfix_20260309_berlin/summary.txt`

Observed result:
- after `wifi_new`:
  - `legacy_task_model_entry_count=3`
  - `legacy_task_model_current_index=2`
- recent `task_get_current_task` samples now resolve to `wifi`, not `timer`
- after scan, queue send/recv first+last roles also resolve to `wifi`
- pre-scan promisc still stayed zero on channels `8/1/6/11`
- wrapped scan still ended at:
  - `scan=ok count=0`

Why this matters:
- the earlier timer-dominance result was a real shim bug, and it is now corrected
- even with a corrected three-task ring (`main`/`timer`/`wifi`) and blob-facing task identity pinned to `wifi` as intended, RX visibility still does not return
- this closes the legacy task-model bookkeeping branch and leaves the deeper circular/run-queue scheduler behavior as the next remaining scheduler-level target

## 2026-03-09: legacy task insertion order still does not restore RX visibility

- Extended the vendored legacy task-model port so newly created tasks are inserted immediately after the current task, matching the legacy circular scheduler semantics:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/legacy_scheduler.rs`

What changed:
- instead of appending new tasks to the end of the diagnostic ring, `note_created_task()` now inserts the new task at `current_index + 1`
- with `main` already present and `timer` precreated, later `wifi` creation now yields the legacy-style ring order:
  - `main -> wifi -> timer`

Validation:
- isolated current standalone comparator with:
  - `MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_ESP_RADIO_TASK_MODEL_DIAG=1`
- summary artifact:
  - `logs/esp_radio_nostd_wifi_control_legacytaskmodel_insert_20260309_berlin/summary.txt`

Observed result:
- after `wifi_new`:
  - `legacy_task_model_entry_count=3`
  - `legacy_task_model_current_index=1`
- recent `task_get_current_task` samples still resolve to `wifi`
- after scan, queue send/recv first+last roles still resolve to `wifi`
- pre-scan promisc still stayed zero on channels `8/1/6/11`
- wrapped scan still ended at:
  - `scan=ok count=0`

Why this matters:
- the task ring now matches the legacy insertion order, not just the legacy membership set
- RX visibility still does not return, so the remaining scheduler gap is deeper than ring order alone
- this closes the task-ring ordering branch and leaves the actual run-queue scheduler behavior as the next remaining scheduler-level target
