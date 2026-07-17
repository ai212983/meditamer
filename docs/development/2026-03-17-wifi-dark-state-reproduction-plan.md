# 2026-03-17 Wi-Fi Dark State Reproduction Plan

## Goal
Reproduce the March 6 dark discovery state again on at least one board before any more source reduction or erase-based comparison.

## Why This Plan Exists
- `meditamer_march_6` was dark earlier on 2026-03-17.
- Later on 2026-03-17, pristine March 6 became green on both available boards.
- Source reduction below full-file `prepare_scan.rs` revert is no longer trustworthy without a reproducible dark baseline.
- Full-erase comparison is also blocked until a before/after dark split exists on the same board.

## Success Criteria
One of the following must happen:
- pristine `meditamer_march_6` becomes dark again on one identified board under a recorded workflow, or
- a specific conditioning sequence is identified that reliably flips a board from green to dark.

## Stop Conditions
Stop dark-state reproduction if any of the following is true:
- pristine March 6 remains green across all planned conditioning sequences
- the board only flips dark under uncontrolled or unknown conditions
- the only available differences are off-device environment changes that cannot be repeated deterministically

## Phase 1: Freeze Current Green Baselines
### Step 1.1
Record the latest green pristine March 6 artifact for board `08:3a:8d:82:0b:98`.

### Step 1.2
Record the latest green pristine March 6 artifact for board `e8:6b:ea:fb:f1:74`.

## Phase 2: Replay Earlier Known Preconditions
### Step 2.1
Replay the earlier workflow that previously produced a dark March 6 run on the board that had gone dark before.

### Step 2.2
Repeat with:
- immediate capture
- delayed capture
- with and without TIMESET
- after March 5 predecessor
- after legacy-comparator predecessor

### Step 2.3
Record a one-round artifact for each replay.

## Phase 3: Condition the Board State Intentionally
### Step 3.1
Cycle predecessor images in controlled order:
- pristine March 6
- March 5
- legacy comparator
- pristine March 6 again

### Step 3.2
If still green, add a power-cycle/replug step between image changes.

### Step 3.3
Only if needed, add a full erase sequence, then reflash pristine March 6.

## Phase 4: Decide Whether Reduction Can Resume
### Step 4.1
If a board becomes dark again under a controlled sequence, pin that board and sequence as the active reduction gate.

### Step 4.2
If no board becomes dark again, stop March 5 -> March 6 source reduction and treat the earlier dark state as unrecovered runtime drift.

## Execution Notes
- Use the one-round discovery profile for reproduction speed.
- Do not mix source edits with board-state reproduction.
- Record board MAC and serial port for every run.
- Prefer no-reflash reruns after each conditioning step to check stability.
