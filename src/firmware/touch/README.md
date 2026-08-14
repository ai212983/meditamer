# Touch Module

This folder is the single integration point for touch sampling, normalization, gesture
classification, and touch debug logging.

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

## Current Architecture

- `types.rs`: touch event and sample types.
- `config.rs`: touch constants and channels (`TOUCH_*`).
- `tasks/acquisition.rs`: GPIO36 ownership, controller lifecycle, and raw sample publication.
- `tasks/acquisition/state.rs`: pure acquisition timing state.
- `tasks/pipeline.rs`: normalization and gesture time advancement.
- `lvgl_multitouch.rs`: stable two-slot transition records for LVGL's multi-touch recognizers.
- `scheduling.rs`: acquisition-loop and active-sample latency telemetry.
- `normalize.rs`: continuity + filtering for noisy frames.
- `core.rs`: `statig` gesture engine.
- `debug_log.rs`: UART formatting for touch traces.
- `mod.rs`: adapter from HAL samples to normalized core events.

The existing normalized single-touch engine remains authoritative for taps, long presses, and
one-finger swipes. Raw two-slot reports also enter a bounded LVGL-only lane while both contacts are
active, followed by one terminating report. LVGL recognizes pinch, rotation, and two-finger swipe
gestures from that lane. A queue discontinuity releases all LVGL slots and suppresses the rest of
that physical gesture so dropped reports cannot create a false recognition.

The two-page LVGL carousel contains Home and Multi-gesture pages, navigated by the left/right
buttons. The gesture page displays the completed recognizer type and measurements after the
controller confirms that all contact slots are released, avoiding an e-paper refresh in the middle
of gesture teardown.

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
powers down touch, selects `ButtonOnly`, and displays READY/PRESSED/RELEASED banners. Production
builds keep it `false` and use shared touch/WAKE classification.

Non-touch app files (`display.rs`, `serial.rs`) now consume this module rather than owning touch
implementation details.

## How To Reproduce / Debug

1. Flash:
- `ESPFLASH_PORT=/dev/cu.usbserial-510 scripts/device/flash.sh release`

2. Capture serial output and inspect `touch_event` and `touch_trace` records around bad cases.

3. Send `TOUCHSCHEDRESET` immediately before a workload, then check executor latency with the
   `METRICS` UART command. `TOUCH_SCHED` reports the maximum acquisition-loop and active-sample
   gaps used by the SD SPI concurrency gate.

## Next Session Plan

1. Add host-side replay fixtures for the latest failing patterns (`release_no_swipe` + long-swipe
fragmentation).
2. If failures persist, instrument extra per-frame continuity state from `normalize.rs` and
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
