# Upload Throughput History Part 27

## 2026-03-10: `backend_legacy_port` now enables the legacy timer/runtime path directly

- Wired the backend-level legacy mode flag into the lower runtime layers instead of requiring separate timer-compat knobs:
  - `vendor/esp-radio-0.17.0/src/compat/timer_compat_legacy.rs`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/bootstrap.rs`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/legacy_preempt.rs`
  - `src/firmware/storage/upload/wifi/backend_legacy_port/bootstrap.rs`
  - `src/firmware/storage/upload/wifi/backend_legacy_port/controller.rs`
- Effective result:
  - `MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG=1` now also implies the legacy timer-compat/runtime bootstrap path
  - the backend mode is closer to a true runtime mode, not just a firmware-side wrapper over current `esp-radio`

Validation:
- `cargo check`

Why this matters:
- this removes one of the remaining structural gaps in `backend_legacy_port`
- deeper validation can now target one backend flag instead of a pile of separate diagnostic knobs

## 2026-03-10: bundled backend_legacy_port runtime now advances past prior panics and stalls after init_tasks precreate

- Validation log: `logs/boot_scan_backend_legacy_port_mode_20260310_semfix/monitor_clean.log`
- With `MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG=1`, the unified backend mode now gets past the earlier
  `legacy_builtin_scheduler::delete_task` crash and the later timer `TaskState::Ready` assertion.
- It reaches:
  - `upload_http: wifi_backend name=backend-legacy-port`
  - `upload_http: legacy_port_runtime name=backend-legacy-port ...`
  - `upload_http: legacy_port init_tasks_precreate_timer_task result=ok`
- After that, serial output stalls completely during the bounded monitor window.
- So the current true backend port has moved the failure boundary forward: it is no longer crashing at the
  prior scheduler/timer assertions, but it is still not reaching Wi-Fi runtime init or boot-scan diagnostics.
- Next focus should be the code immediately after `init_tasks_precreate_timer_task`, not the earlier panic paths.

## 2026-03-10: backend_legacy_port now stalls inside `esp_radio::wifi::new(...)`

- Validation log: `logs/boot_scan_backend_legacy_port_mode_20260310_stagecrumbs/monitor_clean.log`
- The true backend mode now reaches:
  - `legacy_port runtime_init stage=before_esp_radio_init`
  - `legacy_port runtime_init stage=after_esp_radio_init`
  - `legacy_port init_tasks_precreate_timer_task result=ok`
  - `legacy_port runtime_init stage=before_wifi_new`
- It then stalls with no further serial output during the bounded monitor window.
- This pins the current backend-port boundary to `esp_radio::wifi::new(...)`, not to the earlier scheduler/timer panic sites.

## 2026-03-10: real per-task thread semaphores unblock `backend_legacy_port` past `esp_wifi_init_internal(...)`, but RX still stays dark

- The legacy built-in scheduler state in vendored `esp-rtos` previously exposed per-task thread
  semaphores as raw `u32` slots, while the current semaphore implementation expected real
  `Semaphore` objects.
- Ported fix:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/legacy_builtin_scheduler.rs`
  - legacy current-task thread semaphores now allocate and return real counting semaphores
    (`Semaphore::new_counting(0, 1)`) instead of raw integer storage.
- Validation log:
  - `logs/boot_scan_backend_legacy_port_mode_20260310_110951_threadsemfix/monitor_clean.log`
- Key effect:
  - the main-thread semaphore give now succeeds
  - `backend_legacy_port` advances through:
    - `esp_wifi_init_internal.after`
    - `esp_wifi_set_mode_null.after`
    - `esp_supplicant_init.after`
    - `esp_wifi_set_tx_done_cb.after`
    - `esp_wifi_internal_reg_rxcb_sta.after`
    - `esp_wifi_internal_reg_rxcb_ap.after`
    - `esp_radio: wifi_new stage=done`
    - `boot_scan_only_diag start=ok`
- But the remaining behavior is still bad:
  - pre-scan promisc stays zero on channels `8/1/6/11`
  - the next visible boundary moves to `wifi_scan_start_process`
  - after that, the backend still remains RX-dark rather than discovering APs
- This closes the old init deadlock as the primary blocker for `backend_legacy_port`.
- The new blocker is later: a fully initialized backend/runtime that still does not admit RX
  traffic before scan completion.

## 2026-03-10: forcing wrapped `backend_legacy_port` scans through direct explicit IDF scan does not restore admission

- Validation log:
  - `logs/boot_scan_backend_legacy_port_mode_20260310_160737_force_direct_explicit_fixed/monitor_retry.log`
- Added a narrow backend override so wrapped `backend_legacy_port` scans use a direct blocking
  `esp_wifi_scan_start(&wifi_scan_config_t, true)` explicit active-scan path instead of the normal
  `esp-radio` wrapper.
- Result:
  - pre-scan promisc still stays zero on channels `8/1/6/11`
  - direct IDF `NULL` scan still returns `ap_num=0`
  - direct IDF explicit scan still returns `ap_num=0`
  - wrapped backend scan now also returns `result_count=0` through that same direct explicit path
  - raw `ScanDone` list remains empty for all paths
- Important runtime detail:
  - legacy timer compat remains active (`setfn_count=6 arm_count=65 exec_count=39`)
  - but the deeper timer-runtime counters are still flat
    (`entry_count=0 resume_count=65 loop_count=16400 mark_ready_count=0 pop_count=0 selected_count=0`)
- This closes “wrapped scan config / wrapper path mismatch” as the next likely cause on the current
  corrected `backend_legacy_port` branch.
- The remaining boundary stays in the legacy timer/runtime execution path after init, not in
  frontend scan invocation shape.

## 2026-03-10: symmetric raw-promisc controls prove the current stack is RX-dark below wrapper level

- Current isolated `esp-radio` standalone was switched from its `Sniffer` helper to the same raw
  promiscuous callback path used in firmware diagnostics:
  - `esp_wifi_get_promiscuous`
  - `esp_wifi_set_promiscuous_rx_cb`
  - `esp_wifi_set_promiscuous_filter`
  - `esp_wifi_set_promiscuous(true/false)`
- Current-stack validation:
  - `logs/esp_radio_nostd_wifi_control_rawpromisc_correctelf_flashonly_20260310_163856/summary.txt`
  - per-channel windows on `8/1/6/11` all stayed at `total=0`
  - aggregate promiscuous counters stayed `0`
  - the wrapped scan still ended at `scan=ok count=0`
- Working legacy no-std standalone was rebuilt on the correct Xtensa toolchain and validated with the
  same raw promiscuous callback path:
  - `logs/esp_wifi_legacy_nostd_control_rawpromisc_manual_20260310_164438/summary.txt`
  - channel `8`: `total=5 mgmt=3 data=2`
  - channel `1`: `total=2 mgmt=2`
  - channel `6`: `total=2 mgmt=2`
  - channel `11`: `total=1 mgmt=1`
  - aggregate promiscuous counters: `total=10 mgmt=8 data=2`
  - then `scan=ok count=5`
- This closes the last wrapper-level ambiguity:
  - the failing current stack is RX-dark even with the same raw promiscuous callback path
  - the working legacy no-std stack sees packets and scans successfully with that exact same path
- The remaining boundary is below wrapper/direct promiscuous setup and below wrapper/direct scan
  invocation, in the current `esp-radio` / runtime RX-admission path itself.

## 2026-03-10: raw-promisc ISR-window comparison shows interrupts advance on both stacks, but only legacy surfaces packets

- Added `wifi_mac_isr` checkpoints around the same raw-promiscuous window in both isolated controls.
- Current failing stack:
  - `logs/esp_radio_nostd_wifi_control_rawpromisc_isr_20260310_165216/summary.txt`
  - `wifi_mac_isr_diag label=before_promisc count=0`
  - `wifi_mac_isr_diag label=after_promisc count=7`
  - `wifi_mac_isr_diag label=after_scan count=36`
  - packet visibility during the same promisc window stayed `0`
- Working legacy stack:
  - `logs/esp_wifi_legacy_nostd_control_rawpromisc_isr_20260310_165306/summary.txt`
  - `wifi_mac_isr_diag label=before_promisc count=0`
  - `wifi_mac_isr_diag label=after_promisc count=8`
  - `wifi_mac_isr_diag label=after_scan count=44`
  - packet visibility during the same promisc window was non-zero on all sampled channels
- This is a stronger boundary than the earlier raw-promisc control alone:
  - the current stack is not dark because `WIFI_MAC` interrupts are absent
  - interrupts advance during the raw-promisc window on both stacks
  - the divergence is lower: packet admission/delivery to the promiscuous callback path itself
- The remaining target is now between `WIFI_MAC` interrupt activity and packet delivery to
  promiscuous/scan admission, not higher-level scheduling or wrapper API shape.

## 2026-03-10: vendoring plan documented to stop hook-level strategic drift

- Added the anti-drift source of truth:
  - `docs/development/wifi-legacy-vendoring-plan.md`
- Wired references from:
  - `docs/development/wifi-upload-decision-ledger.md`
  - `docs/development/README.md`
  - `docs/development/upload-throughput-history.md`
- Purpose:
  - freeze the strategic decision that future Wi-Fi work should advance the
    true `backend_legacy_port` / vendored legacy runtime path
  - prevent further default drift back into generic `esp-radio` hook A/B work

## 2026-03-10: `backend_legacy_port` now uses explicit vendored legacy `init_tasks()` bootstrap

- Replaced the remaining generic timer-task precreate in the legacy backend path with an explicit
  vendored `init_legacy_wifi_tasks()` helper:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/legacy_tasks.rs`
  - `src/firmware/storage/upload/wifi/backend_legacy_port/runtime.rs`
  - `src/firmware/storage/upload/wifi/backend_legacy_port/controller.rs`
- That helper now creates the legacy Wi-Fi timer task directly and yields once, matching the old
  `esp-wifi 0.15.1` `tasks::init_tasks()` contract more closely than the old
  `precreate_esp_radio_timer_task()` shortcut.
- Ported the next slice of legacy runtime behavior by giving the helper a dedicated
  `legacy_timer_task` entrypoint in:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/timer_queue.rs`
- Also split the oversized vendored `esp_radio` runtime entrypoint so the port can keep moving
  without growing a monolith:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/policy.rs`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/task_create_diag.rs`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/semaphore_impl.rs`
- Build status:
  - `cargo check` passes

Why this matters:
- This is a true `backend_legacy_port` / vendored-runtime port step, not a new generic hook A/B.
- The remaining gap is now the behavior of the ported legacy timer/runtime itself, not whether the
  backend still hides behind generic timer-task helpers.


## 2026-03-10 Raw Promisc and Alloc Follow-up

- Ported the legacy `g_misc_nvs` assignment into current `esp-radio` `wifi_init()` for `esp32/esp32s3`.
- Validated isolated current raw-promisc control at `logs/esp_radio_nostd_wifi_control_gmiscnvs_20260310_berlin/summary.txt`.
- Result unchanged: promisc enable latched, `WIFI_MAC` interrupts advanced, packet delivery stayed zero, `scan=ok count=0`.
- Then completed the missing legacy alloc parity in current `wifi` OS adapter by routing `zalloc_internal`, `wifi_calloc`, and `wifi_zalloc` through plain `calloc` under the existing legacy alloc knob.
- Validated isolated current raw-promisc control at `logs/esp_radio_nostd_wifi_control_legacyalloc2_20260310_berlin/summary.txt`.
- Result unchanged again: zero raw-promisc packets, `WIFI_MAC` interrupts advanced, `scan=ok count=0`.
- This closes two more low-level deltas: `g_misc_nvs` assignment and incomplete legacy calloc/zalloc routing are not sufficient causes of the RX-dark state.

## 2026-03-10 Current standalone proves the boundary is before `recv_cb_sta/ap`

- The existing isolated current standalone log already contains the decisive callback-side evidence:
  - `logs/esp_radio_nostd_wifi_control_gmiscnvs_20260310_berlin/monitor.log`
- In that run:
  - `wifi_mac_isr_diag label=after_promisc count=11`
  - `wifi_mac_isr_diag label=after_scan count=39`
  - `wifi_rx_cb_diag label=after_wifi_new sta=0 ap=0`
  - `wifi_rx_cb_diag label=after_wifi_start sta=0 ap=0`
  - `wifi_rx_cb_diag label=after_scan sta=0 ap=0`
  - raw promisc still saw `total=0`
  - wrapped scan still ended at `scan=ok count=0`
- This tightens the surviving boundary again:
  - interrupts are alive
  - internal STA/AP RX callbacks never fire
  - the failure is therefore earlier than `recv_cb_sta/ap` queueing and earlier than scan/BSS admission
- The next concrete target is the first blob-facing packet delivery step before `esp_wifi_internal_reg_rxcb`-registered callbacks become visible, not more scan-wrapper or promisc-wrapper A/Bs.

## 2026-03-10 Legacy `queue_send_from_isr` semantics are the first branch that restores packet delivery on current `esp-radio`

- Added a blob-facing A/B in current `esp-radio` OS adapter:
  - `MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_QUEUE_SEND_FROM_ISR_DIAG=1`
- This forces current `queue_send_from_isr` to behave like the working legacy path:
  - use normal queue send semantics
  - always set `higher_priority_task_waken = 1`
- Validated on the isolated current standalone:
  - `logs/esp_radio_nostd_wifi_control_legacyisrsend_apponly_20260310_183925/monitor_retry2.log`
- Result:
  - raw promisc packet delivery is no longer zero
  - sampled channels show non-zero packet visibility again:
    - channel `8`: `total=2 mgmt=2`
    - channel `1`: `total=2 mgmt=2`
    - channel `11`: `total=1 mgmt=1`
    - aggregate `total=5 mgmt=5`
  - `wifi_mac_isr` still advances normally during the same window
- This is the first current-stack branch that materially changes the RX-dark boundary in the right direction.
- However, that standalone run then panicked before scan completion because queue diagnostics still called
  `preempt::current_task()` in a context where no current task was set.
- That panic was diagnostic-induced, not proof that restored packet delivery regressed.
- The next required step is hardware validation of the same ISR-send branch on a smaller main-firmware image
  or on a standalone image after the diagnostic-path panic is removed.

## 2026-03-10 `backend_legacy_port` plus legacy `queue_send_from_isr` semantics still stays RX-dark on the slim main firmware image

- Built a smaller main firmware image with:
  - `wifi-debug-slim-app`
  - `MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG=1`
  - `MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_QUEUE_SEND_FROM_ISR_DIAG=1`
  - boot-scan diagnostics enabled
- The app-only payload shrank to:
  - `1,118,896` bytes (`27.10%` of the app partition)
- Validation log:
  - `logs/boot_scan_backend_legacy_port_legacyisrsend_slim_20260310_berlin/monitor_espflash.log`
- Result:
  - backend mode is active:
    - `legacy_port_runtime name=backend-legacy-port`
  - boot scan starts cleanly:
    - `boot_scan_only_diag start=ok`
  - pre-scan promisc still stays zero on channels `8/1/6/11`
  - wrapped scan still returns `result_count=0`
  - direct IDF `NULL` and direct IDF explicit scans both still return `ap_num=0`
  - `wifi_mac_isr_count` still advances:
    - `after=rust_scan count=34`
    - `after=idf_compare count=65`
    - `after=idf_explicit_compare count=97`
  - internal RX callbacks still stay dark:
    - `wifi_rx_cb_count after=rust_scan sta=0 ap=0`
    - `wifi_rx_cb_count after=idf_compare sta=0 ap=0`
    - `wifi_rx_cb_count after=idf_explicit_compare sta=0 ap=0`
- This is an important split:
  - isolated current standalone `esp-radio` changes positively under legacy ISR-send semantics
  - main firmware `backend_legacy_port` does not
- So the next target is the exact runtime difference between those two current-generation paths, not more generic scan/promisc wrapper A/Bs.


_Continued in [Part 27, continuation 2](./part-27-02.md)._
