# Touch Engine Plan

## Objective

Build a reliable panel-touch and gesture engine for Inkplate using `Embassy` task separation and a `statig` classifier, with explicit filtering stages and test coverage.

## Constraints

- Keep behavior representative of production runtime (no wizard-only timing hacks).
- Prefer logic-based robustness over debounce-only tweaks.
- Preserve observability (serial logs, counters, trace fields).

## Phase Plan

### Phase 1: Source-Aligned Filter Pipeline (Current)

Deliverables:
- Document external sources and borrowed patterns in `src/firmware/touch/README.md`.
- Refactor normalization into explicit staged filtering.
- Add unit tests for dropout continuity, slot instability, and outlier handling.

Validation:
- Host tests in `src/firmware/touch/normalize.rs` and `src/firmware/touch/core.rs` pass.
- Wizard step 1 touch tracking stable across repeated taps and drags.

### Phase 2: Classifier Hardening

Deliverables:
- Tighten `statig` gesture transitions for long swipes and release/recontact continuity.
- Reduce false split-swipes by improving release finalization logic.
- Keep direction symmetry (left/right/up/down) with measurable parity.

Validation:
- Swipe matrix in wizard step 4 shows comparable success rates by direction.
- Trace counters show reduced `up dur~0 dx=0 dy=0` collapses.

### Phase 3: Embassy Sampling/Render Decoupling

Deliverables:
- Dedicated touch sampling task and queue.
- Non-blocking render pipeline with bounded frame budget and partial redraw priority.
- Backpressure-safe event delivery from sampling to UI/classifier.

Validation:
- Touch sample cadence remains stable during redraw-heavy wizard screens.
- Fast and long swipes remain detectable under sustained rendering load.

### Phase 4: Calibration and Diagnostics

Deliverables:
- Calibration output used by runtime transform consistently.
- Session logging utility reusable beyond wizard.
- Deterministic dump generation and parsing checks.

Validation:
- Calibration wizard and runtime touch coordinates match.
- Dump files are complete and parseable across repeated runs.

## Immediate Next Steps

1. Implement first filter-stage improvements in `src/firmware/touch/normalize.rs`.
2. Add tests that exercise outlier rejection vs valid fast motion.
3. Re-run wizard and inspect trace logs before changing classifier thresholds.
