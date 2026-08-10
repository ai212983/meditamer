# Upload Throughput History Part 20

## 2026-03-09: recent sender-side queue ordinals still do not show `0x17`, while receive-side samples do

- Extended the standalone current comparator one more step:
  - `tools/esp_radio_nostd_wifi_control/src/main.rs`
- The tool now prints the existing recent sender-side ring from vendored Wi-Fi OS diagnostics in addition to the receive-side samples.
- Rebuilt the standalone current comparator, generated an app image with `espflash save-image`, flashed app-only to `0x10000` using `esptool.py`, and captured:
  - `logs/esp_radio_nostd_wifi_control_sendrecent_20260309_102821/monitor.log`

Key evidence from the log:
- `after_wifi_new` and `after_wifi_start` remain consistent:
  - sender-side recent entries are ordinary `0x6` control messages
  - receive-side samples consume the same `0x6` control messages
- During the failing scan:
  - recent sender-side ordinals `35..40` show only:
    - `timer -> wifi` `0x7 / 0x8 / 0x0`
    - `wifi -> wifi` `0x0`
    - `wifi -> wifi` `0x10`
    - `main -> wifi` `0x6`
  - receive-side samples still include:
    - `0x6`
    - `0x0`
    - `0x17`
    - `0x17`
    - `0x10`
    - `0x7 / 0x8 / 0x0`
- The scan still ends at:
  - `scan=ok count=0`

Conclusion:
- the newly surfaced receive-side `0x17` family is still not visible in the latest sender-side ordinals around the failing scan tail
- that makes the remaining target even narrower:
  - either `0x17` is produced earlier in the sender stream and only becomes visible at the consumer sample window
  - or `0x17` is introduced/decoded in consumer-side handling rather than being obvious in the late sender-side ring
- the next high-value step is to trace receive ordering/dispatch around `0x17`, not generic queue activity

## 2026-03-09: recent receive-side ordinals confirm `0x17` as a stable late consumer-side event

- Extended the standalone current comparator again:
  - `tools/esp_radio_nostd_wifi_control/src/main.rs`
- Extended vendored Wi-Fi OS diagnostics with a recent receive ring:
  - `vendor/esp-radio-0.17.0/src/common_adapter.rs`
- The receive ring mirrors the sender ring:
  - recent ordinal
  - queue pointer
  - task pointer
  - item/pointee words
- Rebuilt the standalone current comparator, generated an app image with `espflash save-image`, flashed app-only to `0x10000` using `esptool.py`, and captured:
  - `logs/esp_radio_nostd_wifi_control_recvrecent_20260309_103120/monitor.log`

Key evidence from the log:
- `after_wifi_new` and `after_wifi_start` stay symmetrical:
  - recent sender-side ordinals are ordinary `0x6` control messages
  - recent receive-side ordinals consume the same `0x6` control messages on the `wifi` task
- During the failing scan:
  - latest sender-side ordinals still show only:
    - `0x6`
    - `0x0`
    - `0x10`
    - timer `0x7 / 0x8 / 0x0`
  - latest receive-side ordinals `49..54` on the `wifi` task are:
    - ordinal `49`: `0x7 / 0x8 / 0x0`
    - ordinal `50`: `0x0`
    - ordinal `51`: `0x17`
    - ordinal `52`: `0x10`
    - ordinal `53`: `0x7 / 0x8 / 0x0`
    - ordinal `54`: `0x6 / 0x35a`
- The scan still ends at:
  - `scan=ok count=0`

Conclusion:
- `0x17` is not just an artifact of the first receive samples
- it is a stable late consumer-side event in the failing scan tail
- and it still does not appear in the late sender-side ring
- the next target is now very specific:
  - map consumer-side handling/dispatch of receive-side `0x17`
  - compare that progression against the known sender-side families rather than continuing generic queue tracing

## 2026-03-09: receive-side `0x17` maps to `ppTask` opcode 23 and dispatches into `lmacProcessTxComplete`

- Extended vendored Wi-Fi OS diagnostics again:
  - `vendor/esp-radio-0.17.0/src/common_adapter.rs`
- Added dequeue caller-PC capture for recent receive entries.
- Extended the standalone current comparator to print that caller PC:
  - `tools/esp_radio_nostd_wifi_control/src/main.rs`
- Rebuilt the standalone current comparator, generated an app image with `espflash save-image`, flashed app-only to `0x10000` using `esptool.py`, and captured:
  - `logs/esp_radio_nostd_wifi_control_recvcaller_20260309_103646/monitor.log`

Key evidence from the log:
- all recent receive entries, including `0x17`, show the same dequeue caller:
  - `caller_ptr=0x800831e1`
- resolving that against the standalone ELF gives:
  - `0x400831e1 -> ppTask`
- static decode of the `ppTask` jump table in the same ELF shows:
  - opcode `0x00 -> 0x40083286 -> ppProcessTxQ`
  - opcode `0x10 -> 0x4008329c -> ppRxPkt`
  - opcode `0x17 -> 0x400832f1 -> lmacProcessTxComplete`
- the failing scan tail therefore contains consumer-side dequeue and dispatch of:
  - `0x0` through the `ppTask` dispatcher
  - `0x10` through `ppRxPkt`
  - `0x17` through `lmacProcessTxComplete`
  - timer `0x7 / 0x8 / 0x0`
- the scan still ends at:
  - `scan=ok count=0`

Conclusion:
- the late receive-side `0x17` family is no longer an unknown opcode
- in the current standalone failing stack, it is a `ppTask` queue opcode that dispatches into `lmacProcessTxComplete`
- this changes the next target from “what is `0x17`?” to:
  - why `lmacProcessTxComplete` appears in the failing scan tail
  - whether those TxComplete events are incidental background traffic or a causal interference pattern unique to the regressed current stack

## 2026-03-09: working legacy no-std scan also shows late `0x17`, so `TxComplete` tail traffic is not unique to the regression

- Extended the working legacy standalone comparator to expose queue send/recv recent rings:
  - `tools/esp_wifi_legacy_nostd_control/src/main.rs`
  - local `esp-wifi 0.15.1` instrumentation in `compat/common.rs` and `lib.rs`
- Hardened the legacy queue-item probe to only dereference DRAM-range pointees, matching the current-stack guard.
- Rebuilt the legacy comparator, flashed app-only to `0x10000`, and captured a clean monitor run with:
  - `logs/esp_wifi_legacy_nostd_control_queuediag_steady2_20260309_105433/espflash_clean_monitor.log`

Key evidence from the log:
- the working legacy no-std path still scans successfully:
  - `scan=ok count=4`
- the successful `after_scan` queue tail already contains the same late families seen on the failing current stack:
  - `0x00000017`
  - `0x00000010`
  - `0x00000007 / 0x00000008 / 0x00000000`
  - trailing `0x00000006` control items
- in the successful legacy run, `after_scan` send/recv ordinals `70..77` include:
  - ordinal `70`: `0x17`
  - ordinal `71`: `0x10`
  - ordinal `72`: `0x7 / 0x8 / 0x0`
  - ordinals `73..77`: `0x6` control items
- the successful steady-state loop after `stop()` changes shape again and begins surfacing `0x19`, showing these queue tails are phase-dependent rather than a single fixed failure signature.

Conclusion:
- late `0x17` / `lmacProcessTxComplete` traffic is not unique to the broken current stack
- the failing current stack and the working legacy stack both carry that queue family around scan tail boundaries
- this closes the “`0x17` presence itself is the regression signature” branch
- the next target should move back earlier:
  - compare why the current stack reaches zero pre-scan promisc / zero admission
  - while the legacy no-std stack still produces AP records under broadly similar queue-tail families

## 2026-03-09: working legacy no-std stack sees management traffic in the pre-scan promisc window

- Extended the working legacy standalone comparator with a bounded pre-scan promiscuous sweep:
  - `tools/esp_wifi_legacy_nostd_control/Cargo.toml`
  - `tools/esp_wifi_legacy_nostd_control/src/main.rs`
- Enabled `esp-wifi 0.15.1` `sniffer` support, added a four-channel sweep (`8, 1, 6, 11`) with `120 ms` dwell, and used direct `esp_wifi_set_channel(...)` plus the crate’s `Sniffer` callback to count total / mgmt / ctrl / data / misc packets before `scan_n(16)`.
- Rebuilt the legacy comparator, generated a fresh app image from the new ELF with `espflash save-image`, flashed app-only to `0x10000` with `esptool.py`, and captured a clean PTY monitor session summarized at:
  - `logs/esp_wifi_legacy_nostd_control_promisc_20260309_berlin/monitor_summary.log`

Key evidence from the monitor session:
- the working legacy no-std path still starts and scans successfully:
  - `start=ok`
  - `scan=ok count=6`
- the pre-scan promisc window is not dark at all:
  - channel `8`: `total=8 mgmt=7 data=1`
  - channel `1`: `total=5 mgmt=1 data=4`
  - channel `6`: `total=2 mgmt=2`
  - channel `11`: `total=1 mgmt=1`
  - aggregate: `total=16 mgmt=11 data=5`
- APs discovered immediately afterward include:
  - `<nearby-ssid-1>`
  - `<test-ssid-guest>`
  - `<test-ssid-primary>`

Conclusion:
- this is the cleanest same-board no-std discriminator so far
- the working legacy no-std stack has live pre-scan RX visibility and management-frame admission in the same environment where the current `esp-radio` stack shows:
  - zero pre-scan promisc packets
  - empty raw `ScanDone`
  - zero BSS admission
- the root-cause boundary therefore moves earlier than queue-tail opcode families and tighter than “generic no-std Wi-Fi”
- the primary remaining target is now earliest RX ingress / frame-admission behavior in the current `esp-radio` / `esp-rtos` stack, not later scan retrieval or queue-tail interpretation

## 2026-03-09: isolated current standalone `esp-radio` comparator reproduces the dark pre-scan promisc window

- Extended the standalone current comparator with the same bounded pre-scan promiscuous sweep used on the working legacy comparator:
  - `tools/esp_radio_nostd_wifi_control/Cargo.toml`
  - `tools/esp_radio_nostd_wifi_control/src/main.rs`
  - `tools/esp_radio_nostd_wifi_control/src/promisc_diag.rs`
- Enabled vendored `esp-radio` `sniffer` support, added the same four-channel sweep (`8, 1, 6, 11`) with `120 ms` dwell, generated a fresh app image with `espflash save-image`, flashed app-only to `0x10000`, and captured a clean PTY monitor session summarized at:
  - `logs/esp_radio_nostd_wifi_control_promisc_20260309_berlin/monitor_summary.log`

Key evidence from the isolated current-stack monitor session:
- the standalone current path reaches normal Wi-Fi start:
  - `start=ok`
- the pre-scan promisc window is completely dark on all sampled channels:
  - channel `8`: `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - channel `1`: `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - channel `6`: `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - channel `11`: `total=0 mgmt=0 ctrl=0 data=0 misc=0`
  - aggregate: `total=0 mgmt=0 ctrl=0 data=0 misc=0`
- the isolated current stack then still ends at:
  - `scan=ok count=0`

Conclusion:
- the dark pre-scan window is now reproduced in the isolated standalone current `esp-radio` comparator, not just in the larger firmware boot-scan harness
- combined with the working legacy no-std comparator, this moves the boundary tighter:
  - not board/environment
  - not generic no-std Wi-Fi
  - not larger firmware interference
- the primary remaining target is earliest RX ingress / frame admission in the current `esp-radio` / `esp-rtos` runtime itself

## 2026-03-09: optimizing `esp-rtos` in the standalone current comparator does not restore scan visibility

- Added a standalone current-stack runtime-only A/B in:
  - `tools/esp_radio_nostd_wifi_control/Cargo.toml`
- Promoted `esp-rtos` to `opt-level = 3` in the tool's dev profile while leaving the rest of the current standalone comparator unchanged.
- Rebuilt the tool, generated a fresh app image, flashed app-only to `0x10000`, and captured:
  - `logs/esp_radio_nostd_wifi_control_esp_rtos_opt3_20260309_berlin/monitor.log`
  - concise summary at `logs/esp_radio_nostd_wifi_control_esp_rtos_opt3_20260309_berlin/monitor_summary.log`

Key evidence from the log:
- the current standalone path still initializes cleanly:
  - `begin=true`
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `start=ok`
- runtime object creation is unchanged from the earlier baseline:
  - after `wifi_new`: `task_create_count=1 queue_create_count=1 queue_last_capacity=200 queue_last_item_size=8`
  - after `start`: same counts
- the scan result is still unchanged:
  - `scan=ok count=0`

Conclusion:
- low optimization of `esp-rtos` was a credible runtime-only delta, but it is not sufficient to explain the current blackout
- promoting `esp-rtos` to `opt-level = 3` does not restore visibility or change the current standalone stack's zero-result scan outcome
- the remaining target stays earlier in current `esp-radio` / `esp-rtos` RX-ingress semantics, not generic dev-profile optimization of `esp-rtos`

## 2026-03-09: legacy-style simple semaphore semantics in current `esp-radio` still leave scan dark

- Added a guarded legacy-style counting semaphore path in:
  - `vendor/esp-radio-0.17.0/src/compat/semaphore.rs`
- New diagnostic knob:
  - `MEDITAMER_WIFI_ESP_RADIO_LEGACY_SIMPLE_SEM_DIAG=1`
- The guarded path replaces only semaphore create/take/give/delete behavior with a simple boxed `u32` counter plus `yield_task()` polling, closer to the working legacy `esp-wifi 0.15.1` semantics, while leaving queue behavior untouched.
- Rebuilt the standalone current comparator with the knob enabled, generated a fresh app image, flashed app-only to `0x10000`, and captured:
  - `logs/esp_radio_nostd_wifi_control_legacysimplesem_20260309_berlin/monitor.log`
  - concise summary at `logs/esp_radio_nostd_wifi_control_legacysimplesem_20260309_berlin/monitor_summary.log`

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
- the broader queue/runtime tail remains the same family as the baseline current run.

Conclusion:
- replacing current semaphore semantics with a legacy-style simple counting semaphore is not sufficient to restore scan visibility
- this closes the “current handle-based semaphore implementation is the primary cause” branch
- the remaining boundary stays in earlier current `esp-radio` / `esp-rtos` RX-ingress/runtime semantics, not semaphore behavior alone

## 2026-03-09: legacy-style simple queue semantics in current `esp-radio` still leave scan dark

- Added a guarded legacy-style queue path in:
  - `vendor/esp-radio-0.17.0/src/compat/queue.rs`
- New diagnostic knob:
  - `MEDITAMER_WIFI_ESP_RADIO_LEGACY_SIMPLE_QUEUE_DIAG=1`
- The guarded path replaces current queue create/send/receive/delete behavior with a simple boxed ring-buffer plus `yield_task()` polling, closer to the working legacy `esp-wifi 0.15.1` queue model, while leaving the rest of the current standalone comparator unchanged.
- Rebuilt the standalone current comparator with the knob enabled, generated a fresh app image, flashed app-only to `0x10000`, and captured:
  - `logs/esp_radio_nostd_wifi_control_legacysimplequeue_20260309_berlin/monitor.log`
  - concise summary at `logs/esp_radio_nostd_wifi_control_legacysimplequeue_20260309_berlin/monitor_summary.log`

Key evidence from the log:
- the current standalone path still initializes cleanly:
  - `begin=true`
  - `esp_radio_init=ok`
  - `wifi_new=ok`
  - `start=ok`
- runtime object creation remains unchanged:
  - after `wifi_new`: `task_create_count=1 queue_create_count=1 queue_last_capacity=200 queue_last_item_size=8`
  - after `start`: same counts
- the same queue-role/message-family tail still appears after the scan:
  - `main -> wifi` control `0x6`
  - `wifi -> wifi` `0x0` and `0x10`
  - `timer -> wifi` `0x7 / 0x8 / 0x0`
  - receive-side `0x17`
- the scan outcome is still unchanged:
  - `scan=ok count=0`

Conclusion:
- replacing current queue semantics with a legacy-style simple queue is not sufficient to restore scan visibility
- this closes the “current `QueueHandle` / `QueuePtr` semantics are the primary cause” branch
- the remaining target stays earlier in current `esp-radio` / `esp-rtos` RX-ingress/runtime semantics, before scan admission and independent of this queue-model substitution
