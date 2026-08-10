# Upload Throughput History Part 21

## 2026-03-09: legacy-like ESP32 PHY enable/disable wrapper in current `esp-radio` still leaves scan dark

- Compared the earliest PHY entry points between the working legacy no-std `esp-wifi 0.15.1` stack and the failing current `esp-radio 0.17.0` stack.
- The strongest source delta was:
  - legacy `esp-wifi` `wifi::os_adapter::phy_enable()` routes through a chip-specific ESP32 path that manages calibration / wakeup / digital-register backup semantics
  - current `esp-radio` `wifi::os_adapter::phy_enable()` simply calls `WIFI::steal().enable_phy()` and `phy_disable()` simply drops the PHY ref count
- Added a guarded ESP32-only legacy-style PHY wrapper in:
  - `vendor/esp-radio-0.17.0/src/wifi/phy_legacy_esp32.rs`
  - `vendor/esp-radio-0.17.0/src/wifi/mod.rs`
  - `vendor/esp-radio-0.17.0/src/wifi/os_adapter/mod.rs`
- New diagnostic knob:
  - `MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_PHY_ENABLE_DIAG=1`
- The guarded path keeps current first-enable behavior through `WIFI::enable_phy()`, but restores a legacy-style wakeup / digital-register-store / digital-register-load wrapper around subsequent PHY transitions for ESP32.
- Rebuilt the standalone current comparator with the knob enabled, generated a fresh app image, flashed app-only to `0x10000`, and captured:
  - `logs/esp_radio_nostd_wifi_control_legacyphy_20260309_berlin/monitor.log`
  - concise summary at `logs/esp_radio_nostd_wifi_control_legacyphy_20260309_berlin/monitor_summary.log`

Key evidence from the log:
- the current standalone path still initializes cleanly:
  - `begin=true`
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `start=ok`
- runtime object creation remains unchanged:
  - after `wifi_new`: `task_create_count=1 queue_create_count=1 queue_last_capacity=200 queue_last_item_size=8`
  - after `start`: same counts
- the scan outcome is still unchanged:
  - `scan=ok count=0`
- the same late queue-family tail still appears after the scan:
  - `main -> wifi` `0x6`
  - `wifi -> wifi` `0x0`
  - `wifi -> wifi` `0x10`
  - `timer -> wifi` `0x7 / 0x8 / 0x0`
  - receive-side `0x17`

Conclusion:
- restoring a legacy-like ESP32 PHY wrapper around current `esp-radio` PHY entry points is not sufficient to restore pre-scan visibility or scan admission
- this closes the “current simplified ESP32 PHY enable/disable path is the primary cause” branch
- the remaining target stays in earlier current `esp-radio` / `esp-rtos` RX-ingress/runtime semantics, not just the PHY wrapper shape

## 2026-03-09: combining legacy-style simple semaphores and simple queues still leaves current standalone `esp-radio` dark

- Reused the isolated current standalone comparator and enabled both already-validated diagnostic knobs together:
  - `MEDITAMER_WIFI_ESP_RADIO_LEGACY_SIMPLE_SEM_DIAG=1`
  - `MEDITAMER_WIFI_ESP_RADIO_LEGACY_SIMPLE_QUEUE_DIAG=1`
- Rebuilt the tool, generated a fresh app image, flashed app-only to `0x10000`, and captured:
  - `logs/esp_radio_nostd_wifi_control_legacysimpleboth_20260309_berlin/monitor.log`
  - concise summary at `logs/esp_radio_nostd_wifi_control_legacysimpleboth_20260309_berlin/monitor_summary.log`

Key evidence from the log:
- the current standalone path still initializes cleanly:
  - `begin=true`
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `start=ok`
- runtime object creation remains unchanged:
  - after `wifi_new`: `task_create_count=1 queue_create_count=1 queue_last_capacity=200 queue_last_item_size=8`
  - after `start`: same counts
- the scan outcome is still unchanged:
  - `scan=ok count=0`
- the late queue-family tail also remains the same family as prior current-stack runs:
  - `0x6`
  - `0x0`
  - `0x10`
  - `0x7 / 0x8 / 0x0`
  - receive-side `0x17`

Conclusion:
- the regression is not explained by an interaction between the current queue model and the current semaphore model that only disappears when both are replaced together
- this closes the “legacy simple queue + legacy simple semaphore combination is the missing contract” branch
- the remaining target stays earlier in current `esp-radio` / `esp-rtos` RX-ingress/runtime semantics, not queue/semaphore primitive shape

## 2026-03-09: precreating the current `esp-rtos` timer task before `esp_radio::init()` still leaves the isolated stack dark

- The earlier timer-task-order probe was invalid because the flashed artifact was stale and did not actually contain the `precreate_timer_task=ok` marker.
- Forced a clean standalone current-stack build into a dedicated target directory so the new artifact could be verified by string content before flash.
- Updated the standalone current comparator to precreate the `esp-rtos` timer task unconditionally before `esp_radio::init()`:
  - `tools/esp_radio_nostd_wifi_control/src/main.rs`
- Flashed that verified image app-only to `0x10000` and captured:
  - `logs/esp_radio_nostd_wifi_control_precreatetimer_clean_20260309_berlin/monitor.log`
  - concise summary at `logs/esp_radio_nostd_wifi_control_precreatetimer_clean_20260309_berlin/monitor_summary.log`

Key evidence from the valid log:
- the precreate branch definitely ran:
  - `precreate_timer_task=ok`
  - `after_precreate_timer_task task_create_count=0 queue_create_count=0`
- the current standalone path still initializes cleanly:
  - `begin=true`
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `start=ok`
- runtime object creation still begins only at `wifi_new`:
  - after `wifi_new`: `task_create_count=1 queue_create_count=1 queue_last_capacity=200 queue_last_item_size=8`
  - after `start`: same counts
- the pre-scan promisc window is still completely dark:
  - channel `8`: `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - channel `1`: `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - channel `6`: `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - channel `11`: `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - aggregate: `total=0 mgmt=0 ctrl=0 data=0 misc=0`
- the scan outcome is still unchanged:
  - `scan=ok count=0`

Conclusion:
- creating the current `esp-rtos` timer task earlier is not sufficient to restore pre-scan visibility or scan admission in the isolated current stack
- this closes the “timer task exists too late” branch with a valid standalone run
- the remaining target stays in deeper current `esp-radio` / `esp-rtos` runtime semantics before RX admission, not timer-task creation order alone

## 2026-03-09: legacy and current no-std stacks both keep thread semaphores stable, but current shows higher internal `task_get_current_task` churn

- Extended the patched working legacy `esp-wifi 0.15.1` comparator to expose internal primitive diagnostics comparable to the current standalone `esp-radio` tool:
  - `thread_sem_get_count`
  - first/last thread-semaphore pointer
  - first/last task pointer observed in `thread_sem_get`
  - `task_get_current_task` count and pointer change count
- Rebuilt and flashed the legacy comparator app-only to `0x10000`, then captured:
  - `logs/esp_wifi_legacy_nostd_control_tasksem2_20260309_berlin/monitor.log`
- Compared that against the already-validated current standalone comparator run:
  - `logs/esp_radio_nostd_wifi_control_tasksem_20260309_berlin/monitor.log`

Key evidence:
- Working legacy no-std path:
  - after `wifi_new`: `thread_sem_get_count=7`, `thread_sem_first_ptr=thread_sem_last_ptr=0x3ffb1f5c`, `thread_sem_ptr_change_count=0`
  - after `wifi_new`: `task_get_current_count=41`, `task_get_current_first_ptr=task_get_current_last_ptr=0x3ffb1e7c`, `task_get_current_change_count=0`
  - after successful scan: `task_get_current_change_count=2`, while `thread_sem_ptr_change_count` and `thread_sem_task_change_count` remain `0`
  - the run still sees pre-scan promiscuous traffic and scans successfully (`count=5`)
- Current failing standalone `esp-radio` path:
  - after `wifi_new`: `thread_sem_get_count=7`, `thread_sem_first_ptr=thread_sem_last_ptr=0x3ffb0d8c`, `thread_sem_ptr_change_count=0`
  - after `wifi_new`: `task_get_current_count=41`, `task_get_current_first_ptr=task_get_current_last_ptr=0x3ffb0cb0`, `task_get_current_change_count=0`
  - after zero-result scan: `task_get_current_change_count=4`, while `thread_sem_ptr_change_count` and `thread_sem_task_change_count` remain `0`
  - the run stays fully dark in the pre-scan promisc window and ends at `scan=ok count=0`

Conclusion:
- stable thread-semaphore identity is common to both the working legacy stack and the failing current stack
- some internal `task_get_current_task` churn is also common to both, so the mere existence of task-pointer changes is not a unique failure signature
- the remaining discriminator is narrower:
  - current `esp-radio` shows a larger internal current-task transition count (`4` vs `2`)
  - but the root-cause target now needs to focus on what those extra current-stack transitions are, not whether any task churn exists at all

## 2026-03-09: recent `task_get_current_task` rings stay on `main` in both legacy and current no-std stacks

- Added recent `task_get_current_task` rings to both isolated comparators:
  - current standalone `esp-radio`:
    - `vendor/esp-radio-0.17.0/src/wifi/os_adapter/mod.rs`
    - `tools/esp_radio_nostd_wifi_control/src/main.rs`
  - patched legacy `esp-wifi 0.15.1`:
    - `/Users/dimitri/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/esp-wifi-0.15.1/src/compat/common.rs`
    - `/Users/dimitri/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/esp-wifi-0.15.1/src/lib.rs`
    - `tools/esp_wifi_legacy_nostd_control/src/main.rs`
- Rebuilt and flashed both tools app-only to `0x10000`, then captured:
  - current: `logs/esp_radio_nostd_wifi_control_taskrecent_build_20260309_berlin/monitor.log`
  - legacy: `logs/esp_wifi_legacy_nostd_control_taskrecent_20260309_berlin/monitor.log`

Key evidence:
- Current failing `esp-radio` standalone:
  - after `wifi_new`, the recent ring contains ordinals `34..41`, all `task_ptr=0x3ffb0cb0`, all `task_role=main`
  - after `wifi_start`, the recent ring contains ordinals `55..62`, all `task_ptr=0x3ffb0cb0`, all `task_role=main`
  - after zero-result scan, the recent ring contains ordinals `139..146`, all `task_ptr=0x3ffb0cb0`, all `task_role=main`
  - the run still has zero pre-scan promisc and `scan=ok count=0`
- Working legacy `esp-wifi` standalone:
  - after `wifi_new`, the recent ring contains ordinals `34..41`, all `task_ptr=0x3ffb1ea4`, all `task_role=main`
  - after `wifi_start`, the recent ring contains ordinals `55..62`, all `task_ptr=0x3ffb1ea4`, all `task_role=main`
  - after successful scan, the recent ring contains ordinals `98..105`, all `task_ptr=0x3ffb1ea4`, all `task_role=main`
  - the run still sees pre-scan promisc traffic and `scan=ok count=5`

Conclusion:
- the extra current-stack `task_get_current_task_change_count` is not explained by the recent samples switching from `main` to some other task role
- both stacks' sampled `task_get_current_task` rings stay pinned to `main`
- so the remaining current-stack delta is below simple task-identity sampling:
  - either older unsampled transitions differ
  - or the failure is in runtime semantics that do not surface as a task-role switch in these recent rings

## 2026-03-09: current standalone `esp-radio` still fails under legacy-style Wi-Fi allocation semantics

- Patched current vendored `esp-radio` `wifi/os_adapter` with a guarded allocator A/B:
  - `MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_WIFI_ALLOC_DIAG=1`
  - `malloc_internal()` routes to generic `malloc()`
  - `calloc_internal_wrapper()` routes to generic `calloc()`
  - through the existing call chain, `wifi_malloc()` / `wifi_calloc()` now also use legacy-style heap semantics
- Extended the isolated current standalone comparator to print allocation counters:
  - `tools/esp_radio_nostd_wifi_control/src/main.rs`
- Rebuilt the standalone current comparator with the legacy-allocation toggle, generated an ESP app image with `espflash save-image`, flashed app-only to `0x10000` with `esptool.py`, and captured:
  - `logs/esp_radio_nostd_wifi_control_legacyalloc_20260309_berlin/monitor_summary.log`

Key evidence:
- allocation activity is definitely present under the legacy-allocation toggle:
  - after `wifi_new`: `malloc_internal_count=16`, `calloc_internal_count=2`, `wifi_malloc_count=5`, `wifi_calloc_count=11`
  - after scan: `malloc_internal_count=43`, `calloc_internal_count=2`, `wifi_malloc_count=18`, `wifi_calloc_count=27`, `free_count=49`
- despite that allocator-mode change, the isolated current stack remains fully dark before scan:
  - channels `8/1/6/11`: all `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - aggregate remains `total=0`
- scan outcome is unchanged:
  - `scan=ok count=0`

Conclusion:
- legacy-style Wi-Fi/internal heap allocation is not sufficient to restore pre-scan RX visibility or scan admission in the isolated current `esp-radio` stack
- this closes the “current internal-only Wi-Fi allocation semantics are the primary cause” branch
- the remaining target stays earlier in current `esp-radio` / `esp-rtos` blob-facing RX-ingress/runtime behavior
