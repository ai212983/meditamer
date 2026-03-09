# Upload Throughput History Part 26

## 2026-03-09: real run-queue override plus forced timeslice still does not restore RX visibility

- Extended the vendored legacy scheduler port so `SchedulerState::run_scheduler()` now prefers the legacy task ring for next-task selection and always arms the next timeslice when more than one legacy task is ready:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/legacy_scheduler.rs`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/mod.rs`
  - `vendor/esp-rtos-0.2.0/src/scheduler.rs`

Validation:
- isolated current standalone comparator with:
  - `MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_ESP_RADIO_TASK_MODEL_DIAG=1`
- summary artifact:
  - `logs/esp_radio_nostd_wifi_control_legacyrunqueue_20260309_berlin/summary.txt`

Observed result:
- after `wifi_new`:
  - `legacy_task_model_entry_count=3`
  - `legacy_task_model_current_index=0`
  - `wifi_task_selected_count=9`
- pre-scan promisc still stayed zero on channels `8/1/6/11`
- wrapped scan still ended at:
  - `scan=ok count=0`

Why this matters:
- this is the first real scheduler-level override of next-task selection plus timeslice policy, not just task-ring bookkeeping
- RX visibility still does not return, so the remaining scheduler gap is deeper than run-queue selection and timeslice arming alone

## 2026-03-09: advancing the legacy ring only on actual task selection still does not restore RX visibility

- Corrected the vendored legacy scheduler shim so the legacy task ring no longer advances on every `yield_task()` request; it now advances only when a task is actually selected by the scheduler:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/legacy_scheduler.rs`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/task_bootstrap.rs`

Validation:
- isolated current standalone comparator with:
  - `MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_ESP_RADIO_TASK_MODEL_DIAG=1`
- summary artifact:
  - `logs/esp_radio_nostd_wifi_control_legacyrunqueue_select_20260309_berlin/summary.txt`

Observed result:
- after `wifi_new`:
  - `legacy_task_model_entry_count=3`
  - `legacy_task_model_current_index=0`
  - `wifi_task_selected_count=9`
- pre-scan promisc still stayed zero on channels `8/1/6/11`
- wrapped scan still ended at:
  - `scan=ok count=0`

Why this matters:
- this removes the remaining easy scheduler bookkeeping mismatch between the legacy circular model and the current shim
- RX visibility still does not return, so the remaining gap is deeper than yield-vs-select ring bookkeeping too

## 2026-03-09: bypassing the priority run queue for legacy-task-model readiness regresses before app startup

- Extended the vendored legacy task-model port so, when enabled, esp-radio task creation/resume and scheduler requeue stop using `RunQueue` for readiness bookkeeping and rely on the legacy circular task ring directly:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/legacy_scheduler.rs`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/mod.rs`
  - `vendor/esp-rtos-0.2.0/src/scheduler.rs`

Validation:
- isolated current standalone comparator with:
  - `MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_ESP_RADIO_TASK_MODEL_DIAG=1`
- monitor artifact:
  - `logs/esp_radio_nostd_wifi_control_legacyrunqueue_bypass_20260309_berlin/monitor.log`

Observed result:
- bootloader output was normal
- no application log lines appeared after boot
- this is an earlier regression than the previous scheduler branches, which still reached `begin=true`

Why this matters:
- this closes the first real “bypass the priority run queue” branch as too aggressive in its current form
- it suggests the next port step should not be another partial override inside the existing scheduler path

## 2026-03-09: first real legacy built-in scheduler structures are now ported in-tree

- Added an in-tree source-level port scaffold of the legacy built-in scheduler state and context model:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/legacy_builtin_scheduler.rs`

What it now contains:
- `LegacyContext`
- `LegacyBuiltinSchedulerState`
- main-task allocation
- circular task insertion after current task
- current-task thread semaphore storage
- task-deletion scheduling state

Why this matters:
- the deeper port is no longer just diagnostics and shims on top of the current priority scheduler
- the next integration step can wire this source-level legacy scheduler model directly, instead of adding more compatibility flags to `RunQueue`

## 2026-03-09: letting the in-tree legacy built-in scheduler own actual context switching still does not restore RX visibility

- Extended the in-tree legacy built-in scheduler so it no longer owns only task handles and thread semaphores; it now also owns actual circular-list task switching:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/legacy_builtin_scheduler.rs`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/mod.rs`
  - `vendor/esp-rtos-0.2.0/src/scheduler.rs`

Validation:
- isolated current standalone comparator with:
  - `MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_BUILTIN_SCHEDULER_DIAG=1`
- artifacts:
  - `logs/esp_radio_nostd_wifi_control_legacybuiltin_switch_20260309_berlin/monitor.log`
  - `logs/esp_radio_nostd_wifi_control_legacybuiltin_switch_20260309_berlin/summary.txt`

Observed result:
- startup still reached:
  - `begin=true`
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `start=ok`
  - `stop=ok`
- wrapped scan still ended at:
  - `scan=ok count=0`
- the scan-tail queue families stayed materially unchanged:
  - `0x6`
  - `0x10`
  - `0x7 / 0x8 / 0x0`
  - consumer-side `0x17`

Why this matters:
- this is the first branch where the new in-tree legacy built-in scheduler owns real task context switching rather than just task-handle APIs
- RX visibility still does not return, so the remaining gap is deeper than the first source-level legacy built-in scheduler switch path too

## 2026-03-09: porting the old `preempt_builtin` Xtensa timer/multitasking slice still does not restore RX visibility

- Ported the first real slice of the working legacy `preempt_builtin` layer into vendored runtime support:
  - legacy Xtensa interrupt-mask setup in `task/xtensa.rs`
  - legacy-style periodic timeslice tick behavior in `timer/mod.rs`
  - explicit runtime knob surface in `esp_radio/mod.rs`

Files:
- `vendor/esp-rtos-0.2.0/src/task/xtensa.rs`
- `vendor/esp-rtos-0.2.0/src/timer/mod.rs`
- `vendor/esp-rtos-0.2.0/src/esp_radio/mod.rs`

Validation:
- isolated current standalone comparator with:
  - `MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_PREEMPT_BUILTIN_TIMER_DIAG=1`
- artifacts:
  - `logs/esp_radio_nostd_wifi_control_legacypreempttimer_20260309_berlin/monitor.log`
  - `logs/esp_radio_nostd_wifi_control_legacypreempttimer_20260309_berlin/summary.txt`

Observed result:
- startup still reached:
  - `begin=true`
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `start=ok`
  - `stop=ok`
- wrapped scan still ended at:
  - `scan=ok count=0`
- scan-tail queue families stayed materially unchanged:
  - `0x6`
  - `0x10`
  - `0x7 / 0x8 / 0x0`
  - consumer-side `0x17`

Why this matters:
- this closes the first real source-level port of the old `preempt_builtin` timer/multitasking layer as insufficient
- the remaining gap is deeper than:
  - task-entry shape
  - scheduler bookkeeping
  - legacy built-in scheduler task-handle substitution
  - legacy built-in scheduler switching
  - legacy Xtensa interrupt-mask and periodic timeslice behavior

## 2026-03-09: routing esp-radio scheduler-facing task handles through the first legacy built-in scheduler state still does not restore RX visibility

- Wired a new diagnostic path so current `esp-radio` task creation/current-task/thread-semaphore/task-deletion calls can route through the in-tree legacy built-in scheduler state:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/legacy_builtin_scheduler.rs`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/mod.rs`

Validation:
- isolated current standalone comparator with:
  - `MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_BUILTIN_SCHEDULER_DIAG=1`
- artifacts:
  - `logs/esp_radio_nostd_wifi_control_legacybuiltin_20260309_berlin/monitor.log`
  - `logs/esp_radio_nostd_wifi_control_legacybuiltin_20260309_berlin/summary.txt`

Observed result:
- startup still reached:
  - `begin=true`
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `start=ok`
- wrapped scan still ended at:
  - `scan=ok count=0`
- scan-tail queue families stayed materially the same:
  - `0x6`
  - `0x10`
  - `0x7 / 0x8 / 0x0`
  - consumer-side `0x17`

Why this matters:
- this is the first source-level branch where esp-radio scheduler-facing task handles stop using the normal current-task/thread-semaphore path and start using a dedicated legacy built-in scheduler state surface
- RX visibility still does not return, so the remaining gap is deeper than the first legacy built-in scheduler state substitution too
