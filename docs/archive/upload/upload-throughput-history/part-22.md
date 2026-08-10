# Upload Throughput History Part 22

## 2026-03-09: current standalone `esp-radio` still fails after adding legacy `phy_mem_init()` at top-level init

- Added a guarded current-stack init A/B for ESP32:
  - `MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_PHY_MEM_INIT_DIAG=1`
  - `esp_radio::init()` now optionally calls the legacy ESP32 `phy_mem_init()` helper before `setup_radio_isr()`
- Rebuilt the isolated current standalone comparator with that toggle, generated an ESP app image with `espflash save-image`, flashed app-only to `0x10000` with `esptool.py`, and captured:
  - `logs/esp_radio_nostd_wifi_control_legacymphyinit_20260309_berlin/monitor_summary.log`

Key evidence:
- startup still stays clean:
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `start=ok`
- pre-scan promisc remains fully dark:
  - channels `8/1/6/11`: all `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - aggregate remains `total=0`
- scan result remains unchanged:
  - `scan=ok count=0`
- allocator and queue-tail shape remain effectively the same:
  - after scan: `malloc_internal_count=43`, `wifi_malloc_count=18`, `wifi_calloc_count=27`, `free_count=49`

Conclusion:
- adding the legacy ESP32 `phy_mem_init()` step to current top-level init is not sufficient to restore pre-scan RX visibility or scan admission
- this closes the “missing legacy `phy_mem_init()` in current `esp-radio::init()`” branch
- the remaining target stays in deeper current `esp-radio` / `esp-rtos` RX-ingress/runtime semantics after init, not this specific PHY-memory setup step

## 2026-03-09: current standalone `esp-rtos` early-stalls under legacy-style timer-loop semantics

- Added a guarded `esp-rtos` timer-loop A/B:
  - `MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_TIMER_LOOP_DIAG=1`
  - in `vendor/esp-rtos-0.2.0/src/esp_radio/timer_queue.rs`, the timer queue now optionally ends each processing pass with `yield_task()` instead of `SCHEDULER.sleep_until(next_wakeup)`, matching the simpler legacy timer-task shape more closely
- Rebuilt the isolated current standalone comparator with that toggle, generated an ESP app image with `espflash save-image`, flashed app-only to `0x10000` with `esptool.py`, and captured:
  - `logs/esp_radio_nostd_wifi_control_legacytimerloop_20260309_berlin/monitor_summary.log`

Key evidence:
- boot output stops immediately after the first scheduler checkpoint:
  - `diag_yield label=after_rtos_start count=8`
  - `rtos_create_diag label=after_rtos_start task_create_count=0 queue_create_count=0`
- the run never reaches:
  - `precreate_timer_task=ok`
  - `esp_radio_init=ok`
  - `wifi_new=ok`

Conclusion:
- legacy-style timer-loop semantics are not a viable drop-in fix for current `esp-rtos`
- they regress the isolated current stack earlier than Wi-Fi bring-up itself
- this closes the “make current timer task loop behave like legacy timer task” branch as a direct fix candidate

## 2026-03-09: isolated legacy vs current no-std init-config snapshots match on effective Wi-Fi init fields

- Added hidden `wifi_init_config_t` / OSI snapshot diagnostics to both isolated tools:
  - current vendored `esp-radio`
  - legacy `esp-wifi 0.15.1`
- Captured:
  - `logs/esp_radio_nostd_wifi_control_initdiag_20260309_berlin/monitor_summary.log`
  - `logs/esp_wifi_legacy_nostd_control_initdiag_20260309_berlin/monitor_summary.log`

Key evidence:
- effective `wifi_init_config_t` field values match across working legacy and failing current:
  - `static_rx_buf_num=10`
  - `dynamic_rx_buf_num=32`
  - `static_tx_buf_num=0`
  - `dynamic_tx_buf_num=32`
  - `rx_mgmt_buf_type=0`
  - `rx_mgmt_buf_num=5`
  - `cache_tx_buf_num=0`
  - `ampdu_rx_enable=1`
  - `ampdu_tx_enable=1`
  - `amsdu_tx_enable=0`
  - `nvs_enable=0`
  - `nano_enable=0`
  - `rx_ba_win=6`
  - `wifi_task_core_id=0`
  - `feature_caps=0x81`
  - `sta_disconnected_pm=false`
  - `tx_hetb_queue_num=3`
  - `dump_hesigb_enable=false`
  - `magic=0x1f2f3f4f`
- both stacks also expose all critical OSI slots as populated/non-null:
  - `_set_isr`
  - `_queue_create`
  - `_queue_recv`
  - `_task_create`
  - `_task_create_pinned_to_core`
  - `_task_get_current_task`
  - `_wifi_thread_semphr_get`
  - `_timer_arm_us`
  - `_event_post`
  - `_malloc_internal`
- despite that:
  - legacy still sees pre-scan traffic and scans:
    - `promisc_diag ... total=14`
    - `scan=ok count=3`
  - current still stays dark and scans zero:
    - `promisc_diag ... total=0`
    - `scan=ok count=0`

Conclusion:
- the effective `wifi_init_config_t` field values are not the discriminator
- simple OSI-table slot presence is not the discriminator either
- the remaining difference is in callback/runtime semantics, not init-config shape

## 2026-03-09: current standalone `esp-radio` still fails with legacy-style `task_delay()`

- Added a guarded current-stack A/B:
  - `MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_TASK_DELAY_DIAG=1`
  - `task_delay()` now optionally uses a legacy-style wait loop:
    - compute blob-tick wait in microseconds
    - repeatedly `yield_task()` until deadline
- Rebuilt the isolated current standalone comparator with that toggle, generated an ESP app image with `espflash save-image`, flashed app-only to `0x10000` with `esptool.py`, and captured:
  - `logs/esp_radio_nostd_wifi_control_legacytaskdelay_20260309_berlin/monitor_summary.log`

Key evidence:
- startup remains clean:
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `start=ok`
- effective init-config fields remain unchanged from the baseline current run
- pre-scan promisc remains fully dark:
  - channels `8/1/6/11`: all `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - aggregate remains `total=0`
- scan result remains unchanged:
  - `scan=ok count=0`

Conclusion:
- current `task_delay()` sleep-vs-yield semantics are not the primary cause of the blackout
- this closes the “legacy-style `task_delay()` restores early RX ingress” branch

## 2026-03-09: legacy vs current isolated no-std `wifi` task creation matches on name/stack/core, but forcing current to max task priority still does not restore RX

- Added a bounded task-create ring at the Wi-Fi adapter boundary in both isolated tools.
- Captured:
  - `logs/esp_wifi_legacy_nostd_control_taskcreate_20260309_berlin/monitor.log`
  - `logs/esp_radio_nostd_wifi_control_taskcreate_20260309_berlin/monitor.log`
  - max-priority A/B:
    - `logs/esp_radio_nostd_wifi_control_forcewifiprio2_20260309_berlin/monitor.log`

Key evidence:
- both stacks create exactly one `wifi` task after `wifi_new`
- matching fields:
  - `name_tag=0x69666977` (`wifi`)
  - `name_len=4`
  - `stack_depth=6656`
  - `core_id=0`
- differing requested priority:
  - legacy wrapper sees `prio=253`
  - current wrapper sees `prio=29`
- legacy still scans successfully after that create:
  - pre-scan `promisc_diag total=12`
  - `scan=ok count=2`
- current still stays dark after that create:
  - pre-scan `promisc_diag total=0`
  - `scan=ok count=0`
- forced-priority A/B proves the current runtime can raise the created `wifi` task to max scheduler priority:
  - `task_create_last_requested_priority=29`
  - `task_create_last_effective_priority=31`
  - but pre-scan promisc remains zero and scan still returns `count=0`

Conclusion:
- simple Wi-Fi task creation shape is not the discriminator:
  - same name
  - same stack
  - same core
- current task priority semantics are also not sufficient to restore RX ingress:
  - forcing the created `wifi` task from requested `29` to effective `31` leaves the blackout unchanged
- this closes the “current Wi-Fi task priority is the primary cause” branch

## 2026-03-09: isolated legacy and current no-std stacks both receive `WIFI_MAC` interrupts, but only current stays RX-dark

- Added isolated `WIFI_MAC` ISR count diagnostics to both standalone no-std tools and captured:
  - `logs/esp_wifi_legacy_nostd_control_isrdiag_20260309_berlin/monitor.log`
  - `logs/esp_radio_nostd_wifi_control_isrdiag_20260309_berlin/monitor.log`

Key evidence:
- working legacy no-std stack:
  - `wifi_mac_isr_diag label=after_wifi_new count=0`
  - `wifi_mac_isr_diag label=after_wifi_start count=0`
  - `promisc_diag total=19 mgmt=8 data=11`
  - `scan=ok count=7`
  - `wifi_mac_isr_diag label=after_scan count=58`
- failing current no-std stack:
  - `wifi_mac_isr_diag label=after_wifi_new count=0`
  - `wifi_mac_isr_diag label=after_wifi_start count=0`
  - `promisc_diag total=0 mgmt=0 data=0`
  - `scan=ok count=0`
  - `wifi_mac_isr_diag label=after_scan count=35`

Conclusion:
- the current isolated `esp-radio` blackout is not caused by a dead `WIFI_MAC` interrupt path
- both isolated stacks show interrupt activity by the time scan completes
- the remaining boundary stays after ISR delivery and before early RX admission / management-frame visibility
- this closes the “current isolated stack never gets `WIFI_MAC` interrupts” branch

## 2026-03-09: legacy `esp-wifi 0.15.1` still scans without `builtin-scheduler`

- Cloned the working isolated legacy tool into:
  - `tools/esp_wifi_legacy_nostd_control_nobuiltin`
- Removed only the `builtin-scheduler` feature from:
  - `tools/esp_wifi_legacy_nostd_control_nobuiltin/Cargo.toml`
- Verified the no-builtin variant still compiles cleanly.
- Flashed app-only and captured:
  - `logs/esp_wifi_legacy_nostd_control_nobuiltin_20260309_berlin/monitor.log`

Key evidence:
- startup remains clean:
  - `init=ok`
  - `wifi_new=ok`
  - `start=ok`
- effective init-config fields remain the same as the working legacy baseline
- pre-scan promisc still sees live traffic:
  - channels `8/1/6/11`: `5 / 1 / 4 / 1`
  - aggregate: `total=11 mgmt=10 data=1`
- scan still succeeds:
  - `scan=ok count=4`
- `WIFI_MAC` ISR count is still live:
  - `wifi_mac_isr_diag label=after_scan count=45`

Conclusion:
- the `builtin-scheduler` feature flag itself is not the primary discriminator
- the old `esp-wifi 0.15.1` stack still admits pre-scan packets and scans without that feature
- this shifts the remaining blame away from “current fails because it lacks `builtin-scheduler`” and toward the newer stack/runtime combination:
  - `esp-radio 0.17.0`
  - `esp-wifi-sys 0.8.1`
  - `esp-hal 1.0.0`
  - `esp-rtos`

## 2026-03-09: isolated `esp-radio 0.16.0` also reproduces the dark pre-scan window

- Added a minimal public-API-only standalone tool:
  - `tools/esp_radio_016_nostd_wifi_control`
- Pinned it to the coherent `0.16.0` family:
  - `esp-radio = 0.16.0`
  - `esp-rtos = 0.1.0`
  - `esp-hal = 1.0.0-rc.1`
  - `esp-wifi-sys = 0.8.1`
  - matching older support crates
- Built, flashed app-only, and captured:
  - `logs/esp_radio_016_nostd_wifi_control_20260309_berlin/monitor.log`

Key evidence:
- startup is clean:
  - `begin=true`
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `set_mode=sta`
  - `start=ok`
- pre-scan promisc is fully dark:
  - channels `8/1/6/11`: all `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - aggregate: `total=0`
- scan still returns zero:
  - `scan=ok count=0`

Conclusion:
- the blackout is not unique to `esp-radio 0.17.0`
- it reproduces one generation earlier in `esp-radio 0.16.0` while still using `esp-wifi-sys 0.8.1`
- that shifts the remaining boundary away from “a 0.17-only `esp-radio` regression” and toward the newer no-std generation shared by:
  - `esp-radio 0.16.x+`
  - `esp-wifi-sys 0.8.1`
  - `esp-rtos`

## 2026-03-09: patching working `esp-wifi 0.15.1` to `esp-wifi-sys 0.8.1` fails at compile time with broad ABI drift

- Created a strict local path override of the working legacy crate:
  - `.scratch/esp-wifi-0.15.1-sys081`
- Patched only its sys dependency to:
  - `esp-wifi-sys = 0.8.1`
- Pointed the working isolated legacy tool at that path override and attempted a clean rebuild.

Key evidence from the failed build:
- Cargo did honor the override:
  - `Adding esp-wifi v0.15.1 (/.../.scratch/esp-wifi-0.15.1-sys081)`
  - `Updating esp-wifi-sys v0.7.1 -> v0.8.1`
- The build then failed inside the patched legacy crate with broad type/layout drift, including:
  - event wrapper breakage for NAN/NDP event structs that are no longer `Copy`/`Clone`
  - `wpa_crypto_funcs_t` field mismatches
  - missing/new fields in:
    - `wifi_scan_config_t`
    - `wifi_ap_config_t`
    - `wifi_scan_threshold_t`
- In other words, this is not a narrow one-field adaptation.

Conclusion:
- the working `esp-wifi 0.15.1` wrapper cannot be moved onto `esp-wifi-sys 0.8.1` as a simple drop-in compatibility test
- the `0.8.1` generation carries broad API/ABI shape changes that are fully consistent with the runtime boundary we have already isolated
- this strengthens the current boundary:
  - working legacy generation: `esp-wifi 0.15.1` + `esp-wifi-sys 0.7.1`
  - failing newer generation: `esp-radio 0.16.x+` + `esp-wifi-sys 0.8.1`

Notes:
- after the experiment, the working legacy tool was restored to its original registry dependency path and verified build-safe again
