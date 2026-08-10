# 2026-03-17 Wi-Fi March 6 Repro Gate Plan

## Goal
Re-establish a deterministic runtime baseline for `meditamer_march_6` before any more March 5 -> March 6 source reduction.

## Why This Plan Exists
- `meditamer_march_5` is currently reproducibly green.
- `meditamer_march_6` was dark earlier on 2026-03-17.
- Later on 2026-03-17, the pristine `meditamer_march_6` snapshot became green again under the same bounded discovery workflow.
- That makes file-level reduction invalid until the March 6 outcome is stable again.

## Success Criteria
One of the following must be achieved:
- `meditamer_march_6` is reproducibly dark again under controlled conditions, or
- `meditamer_march_6` is shown to be reproducibly green now, and the earlier dark run is reclassified as a drifted validation state rather than a stable regression baseline.

## Stop Conditions
Stop source-level reduction if any of the following is true:
- pristine `meditamer_march_6` stays green across repeated controlled reruns
- dark-vs-green outcome depends on uncontrolled preconditions
- the only way to flip the result is by changing device/environment state outside the source tree

## Phase 1: Freeze Current Green March 6 State
### Step 1.1
Record the latest pristine March 6 green artifact.

### Step 1.2
Treat that artifact as the active current baseline until disproven.

## Phase 2: Reproduce the Earlier Dark March 6 Conditions
### Step 2.1
Collect the earlier dark March 6 artifact and the later green March 6 artifact.

### Step 2.2
List the observable differences in execution conditions:
- flashed predecessor image
- whether TIMESET ran
- capture script and hostctl path
- serial-port selection and cached-port state
- number of attached devices
- time elapsed after flash before capture

### Step 2.3
Do not change source while testing these preconditions.

## Phase 3: Re-run March 6 Under Controlled Precondition Matrix
### Step 3.1
Test pristine March 6 after flashing from a March 5 predecessor image.

### Step 3.2
Test pristine March 6 after flashing from the legacy comparator predecessor image.

### Step 3.3
Test pristine March 6 with TIMESET enabled vs disabled.

### Step 3.4
Test pristine March 6 with immediate capture vs delayed capture.

### Step 3.5
Record a bounded artifact for each run.

## Phase 4: Check Device-State Dependence
### Step 4.1
Repeat the pristine March 6 repro gate on the other similar device when available.

### Step 4.2
Classify whether the March 6 outcome follows:
- source only
- flashed predecessor state
- device-specific state
- host-side workflow differences

## Phase 5: Decide Whether Reduction Can Resume
### Step 5.1
If pristine March 6 becomes reproducibly dark again, resume the March 5 -> March 6 reduction plan.

### Step 5.2
If pristine March 6 stays reproducibly green, stop source reduction and focus on validation/runtime drift instead.

## Execution Notes
- Use the current repo capture harness for all repro-gate runs.
- Keep the March 6 source tree pristine during this plan.
- Change one precondition at a time.
- Record the predecessor image before each March 6 flash.

## Next Step
Run a small precondition matrix on pristine `meditamer_march_6`:
- after March 5 predecessor
- after legacy-comparator predecessor
- with TIMESET on and off

## Current Results

### 2026-03-17 matrix slices completed

- `meditamer_march_6` after `meditamer_march_5` predecessor, TIMESET off:
  - green
  - artifact: `logs/march6_repro_gate_after_march5_timeset_off_20260317_102100.log`
- `meditamer_march_6` after legacy-comparator predecessor, TIMESET off:
  - green
  - artifact: `logs/march6_repro_gate_after_legacy_predecessor_timeset_off_20260317_102500.log`
- pristine `meditamer_march_6` with current-repo TIMESET applied before capture:
  - not dark
  - artifact: `logs/march6_repro_gate_timeset_on_20260317_102850.log`
- pristine `meditamer_march_6` with delayed capture on the same device:
  - green
  - artifact: `logs/march6_repro_gate_delayed_capture_20260317_103400.log`
- pristine `meditamer_march_6` on the other similar device:
  - dark
  - artifact: `logs/march6_repro_gate_other_device_20260317_103700.log`
- later on 2026-03-17, the same board that had earlier reproduced dark March 6
  (`08:3a:8d:82:0b:98`) was rechecked with pristine March 6 and stayed green
  across three one-round reruns:
  - `logs/dark_board_pre_erase_pristine_march6_round1_20260317.log`
  - `logs/dark_board_pre_erase_pristine_march6_round2_20260317.log`
  - `logs/dark_board_pre_erase_pristine_march6_round3_20260317.log`

### Current interpretation

- predecessor image alone is not sufficient to recover the earlier dark March 6 state
- TIMESET alone is not sufficient to recover the earlier dark March 6 state
- delayed attach alone is not sufficient to recover the earlier dark March 6 state
- the dark March 6 run was reproducible earlier on the other similar device, but
  is not currently reproducible there either
- the March 6 outcome is therefore drifting over time even on the same board,
  not only across boards

## Updated Next Step

Do not continue source reduction or erase-based device comparison until the dark
March 6 state is deliberately reproduced again on at least one board.

Shift to a dark-state reproduction plan rather than more reduction against an
uncontrolled runtime state.
