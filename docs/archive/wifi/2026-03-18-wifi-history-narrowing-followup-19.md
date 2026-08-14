# Wi-Fi History Narrowing Followup 19

## Scope

This follow-up records the next runtime narrowing step after followups 18 and 17.

Goal:
- determine whether the branch split between the legacy-compat baseline app and the legacy-compat + pair-retarget app happens inside `scan_start`
- or after `scan_start` in the legacy timer / queue phase that follows it

## New Instrumentation

Extended `scan_process_wrap_diag` to snapshot live blob state immediately before and after `scan_start`:
- `g_chm` pointer and key fields
  - `op_chan`
  - `home_chan`
  - `current_chan`
  - `ptr_08`
  - `ptr_0c`
- `g_scan` pointer and key fields
  - `word_00`
  - `word_114`

Also extended legacy timer compat diagnostics with a recent execution ring so the boot-scan runtime logger can report which legacy callbacks actually executed.

## Runs

1. Legacy timer compat baseline app
- log: [flash_capture_20260318_legacycompat_scanstart_state/capture.log](../../logs/flash_capture_20260318_legacycompat_scanstart_state/capture.log)
- log: [flash_capture_20260318_legacycompat_scanstart_execring/capture.log](../../logs/flash_capture_20260318_legacycompat_scanstart_execring/capture.log)

2. Legacy timer compat plus pair retarget
- log: [flash_capture_20260318_legacycompat_pair_retarget_scanstart_state/capture.log](../../logs/flash_capture_20260318_legacycompat_pair_retarget_scanstart_state/capture.log)
- log: [flash_capture_20260318_legacycompat_pair_retarget_execring/capture.log](../../logs/flash_capture_20260318_legacycompat_pair_retarget_execring/capture.log)

## Proven Facts

### 1. Both branches enter the same bad intermediate state inside `scan_start`

Baseline app at `idf_explicit_compare_postcall`:
- `scan_start ret=0`
- `scan_process_wrap_scan_start_state ... pre_op_chan=0xff post_op_chan=0x01`
- `pre_ptr08=0x00000000 post_ptr08=0x0000000a`
- `pre_ptr0c=0x00000000 post_ptr0c=0x00000014`
- `pre_scan_word00=0x00000000 post_scan_word00=0x0000010f`
- `pre_scan_word114=0x00000000 post_scan_word114=0x00000000`

Pair-retarget app at `idf_explicit_compare_postcall`:
- same structural mutation
- `pre_op_chan=0xff post_op_chan=0x01`
- `pre_ptr08=0x00000000 post_ptr08=0x0000000a`
- `pre_ptr0c=0x00000000 post_ptr0c=0x00000014`
- `pre_scan_word00=0x00000000 post_scan_word00=0x0000010f`
- `pre_scan_word114=0x00000000 post_scan_word114=0x00000000`

This closes the earlier uncertainty.

The branch split does not originate from `scan_start` taking different immediate blob mutations.

### 2. Baseline recovers after `scan_start`

Baseline postcall state later in the same explicit compare window:
- `scan_rc=0`
- `scan_done_count=1`
- `blob_chm ... op_chan=0xff`
- `blob_scan ... word_00=0x00000000`
- `blob_scan ... word_114=0x00000080`
- `scan_get_scan_id` reaches `0x80`

So the baseline path leaves the bad intermediate state after `scan_start` and stabilizes into the scan-id / postcall-success branch.

### 3. Pair-retarget stays in the bad state after `scan_start`

Pair-retarget postcall state:
- `scan_rc=12300`
- `scan_done_count=0`
- `blob_chm ... op_chan=0x01`
- `blob_scan ... word_00=0x0000010f`
- `blob_scan ... word_114=0x00000000`
- `scan_get_scan_id` remains `0x00000000`

So the paired retarget path fails to recover from the same intermediate state.

### 4. Baseline executes repeated legacy timer callbacks in the recovery window

Baseline exec-ring summary at `idf_explicit_compare_postcall`:
- `timer_compat_diag ... exec_count=13`
- recent executions are all the same callback family:
  - ordinals `8..13`
  - `callback_ptr=0x40122cf8`
  - `arg_ptr=0x0`

This is the strongest new runtime signal from this iteration.

The baseline recovery phase is accompanied by repeated execution of one legacy callback family after `scan_start` and before the scan-id settles to `0x80`.

### 5. The pair-retarget branch does not show the corresponding postcall exec-ring evidence

In the pair-retarget capture:
- the same postcall state lines are present
- but there are no `timer_compat_diag after=idf_explicit_compare_postcall` or `timer_compat_exec_recent after=idf_explicit_compare_postcall` lines
- `scan_get_scan_id` stays `0x00000000`
- the run remains in the earlier `scan_rc=12300` branch

This does not yet prove that no legacy callback executed at all.
But it does prove that the branch never reaches the same visible recovery regime that the baseline reaches.

## Current Narrowed Boundary

The live branch split is now after `scan_start`, not inside it.

More precisely:
- both branches enter the same bad intermediate `g_chm` / `g_scan` state inside `scan_start`
- only baseline recovers from that state
- baseline recovery correlates with repeated execution of one legacy timer callback family (`callback_ptr=0x40122cf8`, `arg=0` in that build)
- pair-retarget does not show the same postcall recovery evidence and remains in the earlier `scan_rc=12300` branch

## What This Closes

Closed:
- `scan_start` immediate blob mutation as the primary differentiator
- any theory that baseline success comes from `scan_start` itself taking a different direct state path

Still live:
- the legacy timer / queue recovery phase after `scan_start`
- the exact callback family executed there in baseline
- why the pair-retarget branch does not reach the same recovery regime

## Exact Next Step

Instrument the first legacy timer execution consumer more directly in the baseline-vs-pair-retarget window.

Most useful target:
- correlate each `scan_get_scan_id` poll with legacy timer execution progress
- or directly instrument the repeated baseline callback family to identify the function and the state mutation that restores:
  - `op_chan=0xff`
  - `word_00=0`
  - `word_114=0x80`

The current best model is:
- `scan_start` pushes both branches into the same temporary state
- baseline escapes that state via a legacy timer-driven recovery/progression path
- pair-retarget does not
