# Event Engine Developer Guide

## Purpose

This guide explains how to work with the current `statig`-based event engine implementation used by the firmware.

Use this document for day-to-day changes (threshold tuning, state-machine updates, trace analysis), while `docs/development/statig-event-engine-plan.md` remains the migration history/roadmap.

## Current Runtime Flow

The runtime path is:

1. `display_task` samples IMU data (`tap_src`, `int1`, accel, gyro).
2. Raw values are wrapped into `SensorFrame`.
3. `EventEngine::tick` runs feature extraction and HSM transitions.
4. Engine emits `EngineAction`s and `EngineTraceSample`.
5. Firmware executes actions (currently `BacklightTrigger`) and streams trace lines over UART.

Backlight timeline behavior (immediate ON, hold, fade) is intentionally outside the engine and still handled in `src/firmware/runtime/display_task/mod.rs`.

## File Map

- `config/events.toml`
  - Authoritative event tuning values.
  - Edit this first for threshold/timing tuning.

- `build.rs`
  - Parses and validates `config/events.toml`.
  - Generates `EVENT_ENGINE_CONFIG` at build time.
  - Fails build on invalid config values.

- `src/firmware/event_engine/config.rs`
  - Engine config structs.
  - Includes generated config from `OUT_DIR/event_config.rs`.
  - Do not hand-edit generated config output.

- `src/firmware/event_engine/types.rs`
  - Public engine types (`SensorFrame`, `MotionFeatures`, `EngineAction`, `RejectReason`, etc.).

- `src/firmware/event_engine/features.rs`
  - Pure feature/candidate computation (`no HAL access`).
  - Contains source-mask and scoring logic.

- `src/firmware/event_engine/tap_hsm.rs`
  - `statig` HSM (`Idle`, `TapSeq1`, `TapSeq2`, `TriggeredCooldown`, `SensorFaultBackoff`).
  - Transition logic and action emission.

- `src/firmware/event_engine/trace.rs`
  - Internal trace payload shape (`state_id`, `reject_reason`, `candidate_score`, etc.).

- `src/firmware/event_engine/registry.rs`
  - Event registration table for enabled/disabled event kinds.

- `src/firmware/runtime/display_task/mod.rs`
  - Integrates engine in `display_task`.
  - Converts IMU samples to `SensorFrame`, handles `BacklightTrigger`, and writes trace CSV.

## Config Editing Workflow

When tuning detection:

1. Edit values in `config/events.toml`.
2. Run `cargo check`.
3. Flash and capture trace logs.
4. Compare false positives / misses and repeat.

Validation rules enforced in `build.rs` include:

- non-zero timing windows,
- ordering constraints (`min_gap_ms <= max_gap_ms <= last_max_gap_ms`),
- positive thresholds,
- at least one non-zero weight.

If validation fails, Cargo prints an actionable panic message from `build.rs`.

## State Machine Editing Workflow

When changing behavior:

1. Update guards/transitions in `src/firmware/event_engine/tap_hsm.rs`.
2. Keep action semantics stable unless intentional:
   - `BacklightTrigger` means "start light cycle now".
   - `EventDetected` is the generic event bus output.
3. Keep reject reason assignment explicit for every reject path.
4. Run `cargo check` and then hardware smoke tests.

## Trace Output

UART header currently includes:

`tap_trace,ms,tap_src,seq,cand,csrc,state,reject,score,window,cooldown,jerk,veto,gyro,int1,int2,pgood,batt_pct,gx,gy,gz,ax,ay,az`

Key columns for debugging:

- `state`: engine state id (`EngineStateId`).
- `reject`: reject reason id (`RejectReason`).
- `score`: candidate score after feature fusion.
- `window`: ms since previous sequence tap.
- `cooldown`: `1` when trigger cooldown is active.
- `cand` / `csrc`: whether candidate passed and which sources contributed.

## Common Tasks

### Tune triple-tap sensitivity

- Adjust `triple_tap.thresholds.*` and `triple_tap.weights.*` in `config/events.toml`.
- Increase jerk thresholds to reduce false positives.
- Increase `max_gap_ms`/`last_max_gap_ms` for slower tap cadence tolerance.

### Diagnose false positives

- Capture logs and inspect rows where `cand=1` but no intended tap happened.
- Check `csrc`, `score`, `veto`, and `reject` patterns.
- Tighten thresholds or increase gyro veto hold/swing threshold as needed.

### Diagnose misses

- Look for intended tap rows with low `score` or repeated rejects (`Debounced`, `GapTooShort`, `GapTooLong`).
- Relax corresponding timing thresholds or jerk thresholds.

## Commands

Build-only validation:

```bash
cargo check
```

Host-side config regression tests:

```bash
scripts/tests/host/test_event_config_host.sh
```

Host-side event engine state-machine tests:

```bash
scripts/tests/host/test_event_engine_host.sh
```

Host-side tooling lint (clippy):

```bash
scripts/ci/lint_host_tools.sh
```

Flash and capture:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/device/flash.sh release
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/touch/tap_capture.sh logs/tap_trace_test.log
```

Quick trace filtering:

```bash
rg '^tap_trace' logs/tap_trace_test.log
```

## Guardrails

- Keep feature extraction pure (`src/firmware/event_engine/features.rs` should not call HAL).
- Keep runtime orchestration in `display_task`; avoid hardware side-effects inside engine code.
- Keep config-driven thresholds/timing in `config/events.toml`; avoid hardcoding detector constants in the main path.
- Preserve deterministic, traceable reject behavior when adding new logic.
