## 2026-03-19: App is not missing ISR hooks, but current ISR queue/yield semantics are weaker than the working legacy path

### New artifacts

- `logs/flash_capture_20260319_163839/capture.log`
- `logs/flash_capture_20260319_legacy_irqosi/capture.log`
- `logs/flash_capture_20260319_legacy_irqmask/capture.log`

### What changed

- Added raw OSI-slot reporting for the current app runtime so the failing firmware now prints the live `osi_funcs_ptr` and the concrete function pointers used for ISR-critical hooks.
- Added raw OSI-slot reporting in the legacy comparator harness so the working old stack can be compared at the same hook layer.
- Compared old `esp-wifi 0.15.1` and current `esp-radio 0.17.0` source for `queue_send_from_isr`, `task_yield_from_isr`, and the lower RTOS queue/yield implementation.

### What is now proven

1. The failing app is not missing ISR-critical OSI hooks.

In `logs/flash_capture_20260319_163839/capture.log`, the live app prints:
- `osi_funcs_ptr=0x3ffb34fc`
- `set_intr=0x400fcca8`
- `clear_intr=0x400fac74`
- `set_isr=0x400ff104`
- `ints_on=0x400fcb8c`
- `ints_off=0x400fcc18`
- `wifi_int_disable=0x400fb888`
- `wifi_int_restore=0x400fb8dc`
- `task_yield_from_isr=0x400fbcd0`
- `queue_send_from_isr=0x400f7ed8`

Those values map to the current `esp-radio` RAM OSI table (`__ESP_RADIO_G_WIFI_OSI_FUNCS`), not a missing or null hook table.

2. The working comparator still reaches a much richer ISR/RX path.

The working comparator logs still show:
- richer interrupt masks in `hal_mac_interrupt_get_event()` / `hal_mac_interrupt_clr_event()`
- RX-side progression through `lmac_rx_done` and `ppEnqueueRxq`
- rising ISR hook counters in `wifi_isr_hook_diag`

This is visible in:
- `logs/flash_capture_20260319_legacy_irqosi/capture.log`
- `logs/flash_capture_20260319_legacy_irqmask/capture.log`

3. Old and current ISR queue-send semantics are materially different.

Old `esp-wifi 0.15.1` path:
- `queue_send_from_isr()` explicitly writes `*higher_priority_task_waken = 1`
- then reuses the legacy queue ISR path
- source: `~/.cargo/registry/.../esp-wifi-0.15.1/src/wifi/os_adapter.rs`

Current `esp-radio 0.17.0` default path:
- `queue_send_from_isr()` forwards to `crate::compat::queue::queue_try_send_to_back_from_isr(...)`
- the modern RTOS queue implementation ignores the `higher_priority_task_waken` argument completely
- source:
  - `vendor/esp-radio-0.17.0/src/common_adapter.rs`
  - `vendor/esp-radio-0.17.0/src/compat/queue.rs`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/queue.rs`

4. Old and current ISR yield semantics are also materially different.

Old path:
- `task_yield_from_isr()` just performs the legacy scheduler yield directly

Current path:
- `task_yield_from_isr()` defaults to `crate::preempt::yield_task_from_isr()`
- the current RTOS backend turns that into `legacy_scheduler::yield_override(); task::yield_task();`
- this is not a distinct ISR wake primitive; it falls back to the normal task-yield path
- source:
  - `vendor/esp-radio-0.17.0/src/wifi/os_adapter/mod.rs`
  - `vendor/esp-radio-0.17.0/src/preempt_backend.rs`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/mod.rs`

5. This matches the March 10 novelty-gated history.

The repo already records that forcing legacy-style ISR queue-send semantics was the first branch that restored raw packet delivery on the current standalone path, but it was not sufficient in the full firmware path and explicit ISR-yield experiments still left the surviving boundary below the easy ISR wrapper layer.

Sources:
- `docs/development/wifi-upload-decision-ledger.md`
- `docs/development/upload-throughput-history/part-27.md`

### Current boundary

The surviving boundary is now tighter than “missing ISR hooks”.

It is now:
- after current ISR hook registration succeeds
- after current ISR queue-send / ISR-yield hooks are present
- but before the app receives the richer RX/scan interrupt family that the working comparator sees

The strongest live hypothesis is:
- the current hook implementations have weaker ISR wake semantics than the working legacy path
- or the MAC interrupt source/mask setup below those hooks still differs enough that the richer RX-end event family never reaches the current app top half

### What this closes

- “the app forgot to register ISR hooks”
- “the app uses a null or wrong OSI table”
- “there is no meaningful behavioral difference between old and current ISR queue/yield hooks”

### Best next step

Do not rerun the legacy ISR-send knob yet; the novelty gate already records that experiment family.

The next useful step is to compare the old and current ISR hook implementations against the live interrupt-mask evidence and determine which side is still dominant:
- weaker current ISR wake semantics
- or earlier MAC interrupt source/mask setup before `hal_mac_interrupt_get_event()`
