# Wi-Fi History Narrowing Followup 20

## Scope

This follow-up records the next narrowing step after followup 19.

Goal:
- determine what the repeated legacy timer execution regime is doing during baseline recovery
- compare that regime to the pair-retarget branch that stays on `scan_rc=12300`

## New Instrumentation

Extended legacy timer compat execution diagnostics so each recent callback execution also records the blob state immediately before `callback.call()`:
- `pre_op_chan`
- `pre_scan_word00`
- `pre_scan_word114`

This was surfaced through the existing boot-scan runtime log as additional fields on `timer_compat_exec_recent`.

## Runs

1. Legacy timer compat baseline app
- log: [flash_capture_20260319_legacycompat_execstate/capture.log](../../logs/flash_capture_20260319_legacycompat_execstate/capture.log)

2. Legacy timer compat plus pair retarget
- log: [flash_capture_20260319_legacycompat_pair_retarget_execstate/capture.log](../../logs/flash_capture_20260319_legacycompat_pair_retarget_execstate/capture.log)

## Proven Facts

### 1. Baseline recovery is a repeated timer-driven channel progression

Baseline postcall state:
- `scan_rc=0`
- `scan_done_count=1`

Baseline recent legacy callback executions at `idf_explicit_compare_postcall`:
- all are the same callback family
- all run with `arg_ptr=0x0`
- ordinals `8..13`
- callback pointer in this build: `0x40122ef4`

Most important state carried into those executions:
- ordinal 8: `pre_op_chan=0x08`, `pre_scan_word00=0x0000010f`, `pre_scan_word114=0x00000000`
- ordinal 9: `pre_op_chan=0x09`, `pre_scan_word00=0x0000010f`, `pre_scan_word114=0x00000000`
- ordinal 10: `pre_op_chan=0x0a`, `pre_scan_word00=0x0000010f`, `pre_scan_word114=0x00000000`
- ordinal 11: `pre_op_chan=0x0b`, `pre_scan_word00=0x0000010f`, `pre_scan_word114=0x00000000`
- ordinal 12: `pre_op_chan=0x0c`, `pre_scan_word00=0x0000010f`, `pre_scan_word114=0x00000000`
- ordinal 13: `pre_op_chan=0x0d`, `pre_scan_word00=0x0000010f`, `pre_scan_word114=0x00000000`

This is the strongest runtime confirmation so far.

Interpretation:
- baseline does not recover because it avoids the bad intermediate state
- baseline recovers because it repeatedly executes a legacy timer callback while still in that state
- that callback advances channel-manager scan progression through concrete channel values (`0x08` .. `0x0d`) before the scan-id path stabilizes

### 2. Pair-retarget never reaches that timer-driven progression regime

Pair-retarget postcall state:
- `scan_rc=12300`
- `scan_done_count=0`

At the same postcall checkpoint:
- there are no `timer_compat_exec_recent after=idf_explicit_compare_postcall` lines

Interpretation:
- pair-retarget does not just execute a different callback and still progress
- it fails before reaching the same visible legacy timer execution regime that baseline reaches

### 3. This tightens the live boundary again

Followup 19 already showed:
- both branches enter the same bad intermediate state inside `scan_start`

This followup adds:
- baseline escapes that state via repeated timer-driven channel progression
- pair-retarget does not reach that progression phase at all

So the live split is now:
- after `scan_start`
- before or at the first legacy timer execution regime that walks `op_chan` across the active scan channels

## Current Narrowed Boundary

The remaining live boundary is now very specific:
- baseline enters bad scan state
- baseline then executes repeated legacy timer callbacks while `scan_word00=0x10f` and `scan_word114=0`
- those executions advance `op_chan` through the active scan-channel progression
- later, the scan-id path stabilizes and the run succeeds
- pair-retarget never reaches that execution regime and instead stays on the earlier `scan_rc=12300` branch

## What This Closes

Closed:
- any theory that baseline success happens before the timer-driven scan progression begins
- any theory that pair-retarget reaches the same channel-step execution regime and only diverges later

Still live:
- why pair-retarget fails before the first visible legacy timer-driven channel-step progression
- whether the decisive split is:
  - timer scheduling/selection before execution
  - callback identity consumed by the scheduler path
  - or queue/task selection that prevents the legacy callback from running

## Exact Next Step

Instrument the first legacy timer dispatch/selection step that precedes callback execution.

Most useful target:
- `process_due_timer()` selection path and the first callback-dispatch transition
- compare baseline versus pair-retarget on:
  - whether a due timer is found
  - which timer is selected
  - whether execution is skipped before `callback.call()`

The best current model is:
- the root-cause boundary is no longer generic Wi-Fi scan setup
- it is the post-`scan_start` timer-driven scan progression path
- pair-retarget breaks before the first visible legacy channel-step callback execution
