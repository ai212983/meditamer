# Touch Capture-to-Replay Reliability Plan

- Status: Proposed
- Last-reviewed: 2026-08-14

## Objective

Close the remaining evidence gap for two observed one-finger swipe failures:

- a deliberate long swipe ends with the `release_no_swipe` phenotype (an `Up` without the intended
  swipe classification); and
- intermittent zero frames fragment one physical long swipe into multiple interactions.

Turn real device captures of both failures into deterministic host fixtures before changing touch
logic, then prove the narrow fix through replay and the production LVGL input path.

## Scope

The capture-to-replay path is:

- device trace emission: `src/firmware/touch/debug_log.rs`;
- passive capture: `scripts/touch/touch_capture.sh`;
- import and fixture creation: `tools/touch_replay/import_touch_log.py` and
  `scripts/touch/make_touch_fixture.sh`;
- replay: `tools/touch_replay/` and `tools/touch_replay/fixtures/`;
- only if a retained fixture fails: the relevant continuity or release logic under
  `src/firmware/touch/normalize/` and `src/firmware/touch/core/`.

No touch threshold, debounce, or recontact change is in scope until a retained capture reproduces
the failure in `tools/touch_replay`.

## Non-goals

- Do not restore the removed touch wizard or its calibration flow.
- Do not redesign the landed Embassy acquisition/pipeline split or LVGL input integration.
- Do not broaden this work into panel calibration, multitouch recognition, rendering, or generic
  touch tuning.
- Do not accept a timing-only workaround that is not justified by the captured frame sequence.

## Work Plan

### 1. Capture the failures on the production path

Build and flash the release firmware through the canonical wrapper:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-XXXX scripts/device/flash.sh release
```

Record the resulting flash-capture artifact path and firmware identity. Then attach passively and
save raw `touch_trace` plus decoded `touch_event` lines while deliberately reproducing each failure:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-XXXX \
  scripts/touch/touch_capture.sh --mode touch logs/touch_release_no_swipe_YYYYMMDD.log
ESPFLASH_PORT=/dev/cu.usbserial-XXXX \
  scripts/touch/touch_capture.sh --mode touch logs/touch_long_swipe_fragmented_YYYYMMDD.log
```

Keep the smallest complete time window that includes contact start, the zero-frame/recontact
sequence, and final release. Record the intended direction and whether the physical gesture was one
continuous contact; the decoded device result alone does not establish the expected behavior.

### 2. Convert captures into regression fixtures

Create separately named fixtures for the `release_no_swipe` and fragmented-long-swipe captures:

```bash
scripts/touch/make_touch_fixture.sh \
  logs/touch_release_no_swipe_YYYYMMDD.log release_no_swipe_right
scripts/touch/make_touch_fixture.sh \
  logs/touch_long_swipe_fragmented_YYYYMMDD.log long_swipe_fragmented_right
```

Use the actual intended direction in each fixture name; `right` above is a concrete example. Trim
unrelated interactions from each generated trace. Review the generated `*_expected.txt`
against the deliberate physical input and set the intended single swipe result explicitly; when the
device is exhibiting the bug, its decoded `touch_event` stream is evidence of the failure, not the
regression oracle. Add both cases to `tools/touch_replay/run_fixtures.sh`.

Run each new case before editing production logic and retain the failing output. If replay does not
reproduce the device failure, improve capture/import fidelity or instrumentation first; do not tune
the classifier from a non-reproducing fixture.

### 3. Make the narrowest replay-backed correction

Change only the continuity, release, or trace field that the captured sequence demonstrates is
wrong. Preserve prompt real-release behavior and existing tap, long-press, short-drag, diagonal-drag,
four-direction swipe, and multitouch-cancel results. Any added trace field must remain bounded and
must help explain a state transition visible in the retained fixture.

## Acceptance

### Host acceptance

- Each retained failing capture produces one intended directional swipe and no extra tap or second
  interaction in replay.
- The new fixtures are invoked by `tools/touch_replay/run_fixtures.sh`.
- All bundled fixtures and touch-replay tests pass:

```bash
RUSTUP_TOOLCHAIN=stable tools/touch_replay/run_fixtures.sh
rustup run stable cargo test --locked \
  --manifest-path tools/touch_replay/Cargo.toml \
  --target "$(rustup run stable rustc -vV | awk '/^host:/ {print $2}')"
```

### Identified-device and physical acceptance

- Flash the exact release artifact validated on the host and record its identity, serial port, and
  capture-artifact directory.
- On the normal production LVGL input path, repeat the captured motion pattern and verify from both
  the physical interaction and serial `touch_event`/`touch_trace` evidence that each deliberate long
  swipe remains one interaction and produces exactly one swipe in the intended direction.
- Exercise ten left, ten right, ten up, and ten down long swipes across the previously failing
  distance/speed range; observe no `release_no_swipe` result and no split interaction in those 40
  attempts.
- Confirm a real lift still ends promptly and that tap, short drag without swipe, and long press
  remain physically distinguishable. This gate uses the production LVGL UI; the removed wizard is
  not an acceptance surface.

## Completion Condition

Archive this plan only when both real failure captures are retained as fixtures, the full host replay
suite passes, and the identified release artifact has passed the physical production-path gate. If
either phenotype cannot be reproduced, record that result and keep the plan Proposed rather than
claiming the reliability gap closed.
