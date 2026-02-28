# Touch Module

This folder is the single integration point for touch sampling, normalization, gesture
classification, wizard UX, and touch debug logging.

Last major update: 2026-02-23.

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

3. IRQ + periodic fallback works better than pure periodic polling.
- IRQ-triggered burst sampling catches fast gesture starts.
- Idle fallback polling prevents lockups when IRQ edges are missed.

4. Direction detection is mostly good now.
- Recent runs show `class_dir` usually correct (`right`/`down` matching guided cases).
- Most remaining wizard failures are speed-tier mismatches and occasional `release_no_swipe`.

5. Off-target starts must not be counted as true swipe failures.
- Wizard now records out-of-FROM interactions as `skip` instead of poisoning case failure stats.

## Current Architecture

- `types.rs`: touch event/sample types and wizard trace sample formats.
- `config.rs`: touch constants/channels (`TOUCH_*`) and IRQ state.
- `tasks.rs`: IRQ task, touch pipeline task, reset/init helpers.
- `normalize.rs`: continuity + filtering for noisy frames.
- `core.rs`: `statig` gesture engine.
- `wizard.rs`: guided calibration/debug UX and swipe-case tracing.
- `debug_log.rs`: on-device session log capture + UART dump formatting.
- `mod.rs`: adapter from HAL samples to normalized core events.

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
