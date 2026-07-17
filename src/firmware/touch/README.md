# Touch Module

This folder is the single integration point for touch sampling, normalization, gesture
classification, wizard UX, and touch debug logging.

Last major update: 2026-07-16.

## Problem We Are Hunting

Hardware reports intermittent zero frames during a continuous finger gesture. Those short dropouts
can fragment one physical swipe into:

- `tap` / `release_no_swipe`, or
- multiple short swipes, or
- direction-correct but speed-mismatched swipes.

The core challenge is preserving true gesture continuity while still ending touches promptly when
the user actually lifts a finger.

## Findings So Far

1. Controller behavior is bursty.
- We often see `count=1` followed by several zero frames and then valid coordinates again.
- This happens mid-gesture, not only at release.

2. Rendering can starve sampling if done at the wrong time.
- Full redraws during active gesture windows reduce sampling continuity.
- Partial redraws and deferred flushes improve reliability.

3. Event-driven reads plus periodic recovery work better than continuous controller polling.
- GPIO36 assertions trigger immediate reads and an 8 ms classification probe cadence.
- A 250 ms idle recovery read prevents lockups when an assertion is missed.
- Active contacts are sampled at 8 ms (125 Hz), while gesture time advances independently.

4. Direction detection is mostly good now.
- Recent runs show `class_dir` usually correct (`right`/`down` matching guided cases).
- Most remaining wizard failures are speed-tier mismatches and occasional `release_no_swipe`.

5. Off-target starts must not be counted as true swipe failures.
- Wizard now records out-of-FROM interactions as `skip` instead of poisoning case failure stats.

## Current Architecture

- `types.rs`: touch event/sample types and wizard trace sample formats.
- `config.rs`: touch constants and channels (`TOUCH_*`).
- `tasks/acquisition.rs`: GPIO36 ownership, controller lifecycle, and raw sample publication.
- `tasks/acquisition/state.rs`: pure acquisition timing state.
- `tasks/pipeline.rs`: normalization and gesture time advancement.
- `scheduling.rs`: acquisition-loop and active-sample latency telemetry.
- `normalize.rs`: continuity + filtering for noisy frames.
- `core.rs`: `statig` gesture engine.
- `wizard.rs`: guided calibration/debug UX and swipe-case tracing.
- `debug_log.rs`: on-device session log capture + UART dump formatting.
- `mod.rs`: adapter from HAL samples to normalized core events.

The calibration wizard captures four corner observations from each `Down` event's
`contact_x`/`contact_y` fields. Those fields preserve the first physical contact while `x`/`y`
carry the debounce-stabilized position. Once calibrated, the wizard transforms current,
first-contact, and stabilized-start coordinates independently for precision tests and guided
swipes. Starting a new calibration discards the previous transform so a failed retry cannot reuse
stale values.

## Shared GPIO36 Input

Inkplate 4 TEMPERA wires the active-low WAKE button and touchscreen interrupt to the same ESP32
GPIO36 input. The firmware therefore treats GPIO36 as a shared input rather than a touch-only IRQ:

- the acquisition task level-polls GPIO36 every 2 ms and classifies its source through controller reads;
- `input/gpio36.rs` waits through the controller's known zero-frame window before classifying an
  assertion with no decoded contact as a WAKE-button press;
- repeated WAKE-button classifications are debounced;
- the display task receives the classified button event, while ordinary touch samples continue
  through the existing normalization and gesture pipeline.

Critical low-power operation should use `Gpio36Mode::ButtonOnly` after the touchscreen has been
explicitly powered down. In shared mode the electrical source cannot be identified from GPIO36
alone, so classification necessarily depends on reading the touch controller.

For a visual hardware test, set `GPIO36_WAKE_BUTTON_DIAGNOSTIC_ENABLED` to `true`. That opt-in mode
forces the Day UI, powers down touch, selects `ButtonOnly`, and displays READY/PRESSED/RELEASED
banners. Production builds keep it `false` and use shared touch/WAKE classification.

Non-touch app files (`display.rs`, `serial.rs`) now consume this module rather than owning touch
implementation details.

## Known Open Issues

1. Speed buckets in wizard are strict.
- Physical swipe direction can be correct but still fail case due to duration bucket.

2. `release_no_swipe` still appears occasionally.
- Usually from a short interaction where motion was not promoted into swipe before release.

3. Rare trace overflow can happen in long sessions.
- Dump header includes overflow flags; inspect them before trusting counts.

## How To Reproduce / Debug

1. Flash:
- `ESPFLASH_PORT=/dev/cu.usbserial-510 scripts/device/flash.sh release`

2. Run wizard on device.

3. Dump logs:
- `ESPFLASH_PORT=/dev/cu.usbserial-510 scripts/touch/touch_wizard_dump.sh`

4. Quick parse:
- `awk -F',' '/^touch_wizard_swipe,[0-9]/{print $7}' logs/<dump>.log | sort | uniq -c`
- Inspect `touch_event` and `touch_trace` sections around bad cases.

5. Send `TOUCHSCHEDRESET` immediately before a workload, then check executor latency with the
   `METRICS` UART command. `TOUCH_SCHED` reports the maximum acquisition-loop and active-sample
   gaps used by the SD SPI concurrency gate.

## Next Session Plan

1. Keep direction/start/end correctness as primary pass criterion in wizard.
2. Decide whether speed should be:
- strict pass/fail,
- informational only, or
- per-user calibrated.
3. Add host-side replay fixtures for the latest failing patterns (`release_no_swipe` + long-swipe
fragmentation).
4. If failures persist, instrument extra per-frame continuity state from `normalize.rs` and
`core.rs` in dump output for one debug branch.

## Source References

1. `tslib`  
<https://github.com/libts/tslib>

2. LVGL gestures  
<https://docs.lvgl.io/9.4/details/main-modules/indev/gestures.html>

3. Espressif `esp_lcd_touch`  
<https://components.espressif.com/components/espressif/esp_lcd_touch>

4. Zephyr input subsystem  
<https://docs.zephyrproject.org/latest/services/input/index.html>

## Guardrails

- Prefer logic-first robustness over timing-only tuning.
- Do not ignore regressions; fix root cause or explicitly ask the user when uncertain.
- Update tests/fixtures with every behavior change.
