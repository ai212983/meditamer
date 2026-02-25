# Statig Event Engine Migration Plan (Handoff)

Last updated: 2026-02-13 (Europe/Berlin context)

## Goal

Move sensor-event detection from ad hoc logic in `src/firmware/runtime/display_task/mod.rs` into a reusable `statig`-based engine that:

- supports current triple-tap backlight behavior reliably,
- is extensible to non-tap events (pickup, placement, stillness, near/far intent),
- supports declarative event tuning/configuration without detector rewrites for threshold/timing changes,
- is testable offline with recorded sensor traces before flashing firmware.

## Scope

- In scope:
  - LSM6DS3-based event detection refactor into an explicit state machine.
  - Preserve current backlight timeline behavior (immediate ON, hold 3s, fade 2s).
  - Keep UART tap trace logging and improve it for state-machine debugging.
  - Add declarative event configuration (`TOML` source) compiled into firmware config tables.
- Out of scope for this phase:
  - Runtime TOML parsing on device.
  - Arbitrary user-defined expressions that require new primitive feature types not implemented in code.
  - ML inference on-device.
  - Hardware changes (new IMU).
  - APDS/BME/BQ event logic implementation (only define extension points).

## Current Baseline (as of this plan)

- Detection and sequencing logic is embedded in `display_task` in `src/firmware/runtime/display_task/mod.rs`.
- Current detector uses fused signals:
  - LSM tap source bits (`TAP_SRC` axis/single/tap-event),
  - accel jerk,
  - gyro swing veto window,
  - sequence continuity assist for the 3rd tap.
- Trace output currently emits:
  - `tap_trace,ms,tap_src,seq,cand,csrc,jerk,veto,gyro,int1,int2,pgood,batt_pct,gx,gy,gz,ax,ay,az`
- Relevant interfaces already exist in HAL:
  - `lsm6ds3_read_tap_src`, `lsm6ds3_read_motion_raw`, `lsm6ds3_int1_level`, `lsm6ds3_int2_level`
  - `set_brightness`, `frontlight_on`, `frontlight_off`

## Target Architecture

### Data Flow

1. `SensorSampler` (polling in Embassy task) produces `SensorFrame`.
2. `FeatureExtractor` converts raw frame into `MotionFeatures`.
3. `EventEngine` (`statig`) consumes features/events and produces `EngineAction`s.
4. `ActionExecutor` applies actions (backlight trigger, trace emission).
5. `TraceSink` logs engine internals for tuning and replay.
6. `ConfigCompiler` (host/build-time) converts event config into static Rust tables.

### Proposed modules

- `src/firmware/event_engine/mod.rs`
- `src/firmware/event_engine/types.rs`
- `src/firmware/event_engine/features.rs`
- `src/firmware/event_engine/tap_hsm.rs`
- `src/firmware/event_engine/config.rs`
- `src/firmware/event_engine/trace.rs`
- `src/firmware/event_engine/registry.rs`
- `build.rs` (or `xtask`) for config compilation
- `config/events.toml` (authoritative declarative event definitions)
- `src/generated/event_config.rs` (generated; checked in or generated during build)

Keep `display_task` as orchestrator only (sample -> engine -> execute actions).

## Engine Model (Statig)

Use a hierarchical state machine for tap/event sequencing:

- Superstate `Active`
  - `Idle`
  - `TapArmed`
  - `TapSeq1`
  - `TapSeq2`
  - `TriggeredCooldown`
- Superstate `Suppressed`
  - `GyroSuppressed` (ignore motion-only candidates during large swings)
  - `SensorFaultBackoff` (I2C/transient failures)

Inputs to the machine:

- `Tick(MotionFeatures, now_ms)`
- `ImuFault`
- `ImuRecovered`

Outputs/actions:

- `BacklightTrigger`
- `TraceSample`
- `CounterReset { reason }`
- `EventDetected { kind }` (generic event bus hook; start with `TripleTap`)

## Heuristic Policy (inside machine guards/actions)

- Candidate score combines:
  - tap-source confidence,
  - jerk confidence,
  - axis consistency with in-flight sequence.
- Gyro veto:
  - blocks motion-only candidates during/shortly after large swing.
  - does not block explicit strong tap-source evidence.
- Sequence rules:
  - bounded inter-tap timing windows,
  - final-tap slightly larger allowed window if prior sequence is strong,
  - cooldown after successful trigger.

Important: keep detector logic deterministic and branch-reason traceable (every reject path has a reason code).

## Generic Event Extensibility

Define generic event interfaces now even if only triple-tap is enabled:

- `EventKind` enum:
  - `TripleTap`,
  - `Pickup`,
  - `Placement`,
  - `StillnessStart`,
  - `StillnessEnd`,
  - `NearIntent`,
  - `FarIntent`.
- `EngineAction::EventDetected { kind, confidence, source_mask }`

This avoids redesign when APDS/BME/touch signals are added later.

## Configurable Events (TOML -> Static Config)

Preferred approach for this firmware:

- Author event definitions in `config/events.toml`.
- Validate and compile at build time into `src/generated/event_config.rs`.
- Device uses generated static tables (`no_std`, no heap parser cost).

Config should cover:

- Event enable/disable flags.
- Thresholds and timing windows.
- Cooldown and veto windows.
- Action mapping (`BacklightTrigger`, later buzzer/UI actions).

Design constraint:

- TOML config can tune existing primitives and compose predefined guard blocks.
- New primitive feature families still require code additions (intentional for safety/determinism).

## Implementation Phases

### Phase 0: Freeze Baseline + Artifacts

- Save a known baseline log set in `logs/`:
  - expected true triple taps from 3 sides,
  - expected negatives (touch-only, placement-only, swing-only).
- Add one short markdown note with session conditions and counts.

Exit criteria:

- Baseline firmware still builds/flashes.
- At least one reproducible capture per scenario exists.

### Phase 1: Extract Types + Pure Feature Layer

- Introduce `SensorFrame`, `MotionFeatures`, `RejectReason`, `CandidateScore`.
- Move jerk/axis/gyro-veto computation into pure functions (`no HAL access`).
- Add host-side unit tests for feature extraction.

Exit criteria:

- `cargo check` passes.
- Feature tests validate expected outputs for representative samples.

### Phase 1.5: Config Compiler + Schema

- Add `config/events.toml` with current triple-tap behavior encoded as config.
- Add schema validation (range checks, required fields, action compatibility).
- Generate Rust config tables consumed by engine.

Exit criteria:

- Build fails fast on invalid config with actionable errors.
- Generated config reproduces current threshold defaults.

### Phase 2: Add Statig Skeleton (Behavior Parity First)

- Add `statig` dependency and minimal HSM with states/events listed above.
- Wire `display_task` to call machine on each sample tick.
- Read thresholds/timing/action wiring from generated config (no hardcoded detector constants in main path).

Exit criteria:

- No regressions in backlight control timeline.
- Parity replay on baseline logs is within agreed tolerance of old detector.

### Phase 3: Trace-Driven Tuning in New Engine

- Expand trace fields with machine internals:
  - `state_id`, `reject_reason`, `candidate_score`, `window_ms`, `cooldown_active`.
- Replay captured logs through host test harness and tune config there first.
- Only then flash and verify on-device.

Exit criteria:

- Triple taps detected on all target enclosure sides in validation run.
- Touch-only and post-placement false positives reduced vs baseline.

### Phase 4: Generic Event Bus Hooks

- Emit `EventDetected` actions even if only `TripleTap` currently consumed.
- Add placeholder detectors (disabled by config) for pickup/stillness using same engine.
- Support multiple configured event entries that reuse shared primitive features.

Exit criteria:

- Engine API supports additional event kinds without touching tap core logic.

### Phase 5: Hardening

- Add fault/backoff behavior for IMU read errors.
- Ensure monitor/capture scripts remain interruptible and file-based by default for tuning sessions.
- Document tuning protocol in `docs/development/`.

Exit criteria:

- Long capture run without deadlocks/hangs.
- Recovery from transient sensor read failures without manual reset.

## Verification Strategy

### Offline first

- Build a small host replay test that parses `tap_trace` logs and feeds machine ticks.
- Assert:
  - trigger count,
  - trigger timing,
  - false positive bounds for negative scenarios.
- Add config regression tests:
  - fixture TOML -> generated Rust snapshot,
  - semantic validation tests for invalid configs.

### On-device protocol

- Standard script flow:
  - flash: `ESPFLASH_PORT=... ./scripts/flash.sh release`
  - capture: `ESPFLASH_PORT=... ./scripts/tap_capture.sh logs/<name>.log`
- Scenario matrix per firmware:
  - 3x triple taps on each of 3 sides,
  - light touches/no taps,
  - place device after taps,
  - deliberate large swing/no tap.

## Risks and Mitigations

- Risk: state-machine migration changes timing subtly.
  - Mitigation: parity phase before tuning, plus replay tests.
- Risk: overfitting to one enclosure side.
  - Mitigation: per-side capture set and side-balanced acceptance checks.
- Risk: hard-to-debug rejects.
  - Mitigation: mandatory reject reason + state tracing.

## Next Session Checklist

1. Add `statig` crate and create `src/firmware/event_engine/` skeleton.
2. Move current pure calculations (jerk, axis, veto timing helpers) into `features.rs`.
3. Add `config/events.toml` and generator pipeline (`build.rs` or `xtask`) with validation.
4. Introduce `TapHsmConfig` loaded from generated config (not hardcoded constants).
5. Implement minimal HSM transitions for `Idle -> TapSeq1 -> TapSeq2 -> TriggeredCooldown`.
6. Keep existing backlight timeline code unchanged; only change trigger source.
7. Add trace fields for `state_id` and `reject_reason`.
8. Run `cargo check` and flash once for smoke validation.

## Decision log

- Chosen engine: `statig`.
- Reason: `no_std` + HSM model + practical fit for Embassy event loop and future generic sensor events.
- Deferred options: `state-machines` (promising but higher adoption risk), hardware smart-IMU migration.
