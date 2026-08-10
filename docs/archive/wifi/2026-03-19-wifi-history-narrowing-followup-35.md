## 2026-03-19: ESP32 interrupt-hook setup code is identical old vs current; the remaining OSI-layer delta is ISR wake semantics

### New artifacts

- `logs/flash_capture_20260319_163839/capture.log`
- `logs/flash_capture_20260319_legacy_irqosi/capture.log`
- `docs/development/2026-03-19-wifi-history-narrowing-followup-34.md`

### What changed

- Compared old `esp-wifi 0.15.1` and current `esp-radio 0.17.0` source for the ESP32 interrupt-hook setup family:
  - `set_intr`
  - `clear_intr`
  - `ints_on`
  - `ints_off`
  - `set_isr`
- Checked the current app capture again against the working comparator ISR counters.

### What is now proven

1. ESP32 interrupt-hook setup code is effectively identical old vs current.

The compared source bodies match in behavior for:
- `set_intr`
  - both force-bind the Wi-Fi interrupt source to CPU0 via `intr_matrix_set(0, intr_source, intr_num)`
- `clear_intr`
  - both are effectively no-op trace stubs
- `ints_on`
  - both call `chip_ints_on(mask)`
- `ints_off`
  - both call `chip_ints_off(mask)`
- `set_isr`
  - both store `(f, arg)` into `ISR_INTERRUPT_1`
  - both enable `WIFI_MAC` interrupt at priority 1

Relevant source files:
- old:
  - `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/esp-wifi-0.15.1/src/wifi/os_adapter.rs`
  - `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/esp-wifi-0.15.1/src/wifi/os_adapter_esp32.rs`
- current:
  - `vendor/esp-radio-0.17.0/src/wifi/os_adapter/mod.rs`
  - `vendor/esp-radio-0.17.0/src/wifi/os_adapter/esp32.rs`

2. The app still has the hooks and still fails before RX delivery.

From `logs/flash_capture_20260319_163839/capture.log`:
- current app uses live OSI table `osi_funcs_ptr=0x3ffb34fc`
- current app still reaches `queue_send_isr=14`
- current app still does not receive the comparator’s richer interrupt-event family
- current app still completes scan with empty AP list

3. The working comparator still shows much stronger ISR progression.

From `logs/flash_capture_20260319_legacy_irqosi/capture.log`:
- `queue_send_isr=68`
- `task_yield_from_isr=68`
- RX path progresses through `lmac_rx_done` and `ppEnqueueRxq`

4. That leaves only one substantive OSI-layer source difference still standing.

At the OSI layer, the remaining meaningful old-vs-current difference is now:
- ISR queue-send semantics
- ISR yield semantics

From follow-up 34:
- old `queue_send_from_isr()` explicitly marks `higher_priority_task_waken`
- current default path forwards into a modern RTOS ISR queue API that ignores that argument
- old `task_yield_from_isr()` is direct legacy scheduler yield
- current default path falls through `yield_task_from_isr()` into the normal task-yield path

### Current boundary

The surviving boundary is now below interrupt-hook setup and above first delivered RX packet.

Concretely:
- interrupt-hook registration/setup is not the cause
- the live OSI-layer difference is reduced to ISR wake behavior
- if that still is not sufficient, the remaining fault domain is deeper MAC interrupt source/mask generation or admission before `hal_mac_interrupt_get_event()` returns the richer RX/scan event family

### What this closes

- “current app broke `set_intr`/`set_isr` wiring relative to old stack”
- “current app never enabled the `WIFI_MAC` interrupt the old way”
- “old/current difference at this layer is in basic interrupt registration”

### Best next step

Do not spend more time on OSI setup hook code.

The remaining justified options are:
- prove ISR wake semantics are still causally dominant in the full firmware path despite prior novelty-gated results
- or move below the OSI layer and instrument MAC interrupt source/mask generation/admission before `hal_mac_interrupt_get_event()`

Given the history, the second path is currently higher value.
