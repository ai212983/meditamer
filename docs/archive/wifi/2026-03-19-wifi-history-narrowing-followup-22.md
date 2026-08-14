# Wi-Fi History Narrowing Followup 22

## Scope

This follow-up records the corrected retarget-target result after followup 21.

Goal:
- test whether the earlier pair-retarget `scan_rc=12300` branch was caused by a bad internal target pointer
- determine whether a baseline-shaped trampoline changes the legacy-compat and current-timer outcomes

## New Instrumentation

Added a diagnostics-only trampoline target for slot retargeting in `timer_compat_legacy`.

New knob:
- `MEDITAMER_WIFI_NAN_TIMER_SLOT_RETARGET_TRAMPOLINE_DIAG=1`

Instead of jumping to `ieee80211_timer_process + 0xa0`, the retarget now installs a trampoline that reproduces the baseline callback call shape:
- `ieee80211_timer_process(7, 8, arg)`

Static decode of the current build proved the old target was wrong for this purpose:
- baseline callback entry used by the slot timers:
  - `nan_dp_schedule_ndc_start + 0x68`
  - disassembly shows it does:
    - `a10 = 7`
    - `a11 = 8`
    - `a12 = arg`
    - `call8 ieee80211_timer_process`
- old retarget target:
  - `ieee80211_timer_process + 0xa0`
  - disassembly shows it is the unlock/log/return error tail, not the baseline equivalent entry

## Runs

1. Legacy timer compat plus pair retarget plus trampoline target
- log: [flash_capture_20260319_legacycompat_pair_retarget_trampoline/capture.log](../../logs/flash_capture_20260319_legacycompat_pair_retarget_trampoline/capture.log)

2. Current timer substrate plus pair retarget plus trampoline target
- log: [flash_capture_20260319_currenttimer_pair_retarget_trampoline/capture.log](../../logs/flash_capture_20260319_currenttimer_pair_retarget_trampoline/capture.log)

## Proven Facts

### 1. The old `+0xa0` retarget target was a bad target for the intended experiment

Static decode in the current build:
- `nan_dp_schedule_ndc_start + 0x68` at `0x40123c20`:
  - `entry`
  - `mov a12, a2`
  - `movi a11, 8`
  - `movi a10, 7`
  - `call8 ieee80211_timer_process`
- `ieee80211_timer_process + 0xa0` at `0x40134a64`:
  - starts at `wifi_api_unlock`
  - then `wifi_log`
  - then returns `-1`

So the earlier pair-retarget `12300` branch was contaminated by an invalid target choice.

### 2. Under legacy timer compat, the corrected trampoline restores the baseline maintenance loop and recovered branch

In the legacy-compat trampoline run:
- retarget trace shows the new target:
  - `legacy_timer_pair_retarget source=0x4012415c target=0x4010a084`
- the retargeted callback now emits the same callback-side-effect loop:
  - disarm/rearm of both `g_chm` slot timers
  - disarm of the ancillary timers
- repeated due executions advance through the channel progression window
- the run recovers to the same later branch as baseline:
  - `scan_done_list status=0 count=0 scan_id=128`
  - `blob_chm after=rust_scan op_chan=0xff`
  - `blob_scan after=rust_scan word_00=0x00000000`
  - `blob_scan after=rust_scan word_114=0x00000080`
  - `scan_get_scan_id ret=0x00000080`
  - `scan_done_eventpost after=rust_scan count=1`

This is a strong causal result.

It proves the earlier pair-retarget `12300` failure was not because the slot-callback family is impossible to emulate.
It was because the previous retarget target was wrong.

### 3. On the current timer substrate, the corrected trampoline still does not fix the branch

In the current-timer trampoline run:
- slot retarget succeeds:
  - `matched_count=2 retargeted_count=2`
  - `from_callback_ptr=0x4012392c to_callback_ptr=0x40134770`
- but the run still fails at the early branch:
  - `blob_chm after=rust_scan_err op_chan=0x01`
  - `blob_scan after=rust_scan_err word_00=0x0000010f`
  - `blob_scan after=rust_scan_err word_114=0x00000000`
  - repeated `scan_get_scan_id ret=0x00000000`

Most important runtime fact from this run:
- timer runtime counters still never start:
  - `timer_runtime_diag ... entry_count=0`
  - `resume_count=0`
  - `loop_count=0`
  - `selected_count=0`
- timer exec counters also stay zero:
  - `timer_exec_diag ... last_callback_ptr=0x0`
  - `timer_compat_diag ... exec_count=0`

So the corrected callback family is not enough on the current timer substrate because the current timer runtime never enters the dispatch phase in this failing window.

## Current Narrowed Boundary

The live boundary is now:
- not the slot callback family by itself
- not the earlier pair-retarget target artifact
- specifically the current timer-substrate runtime dispatch path

More concretely:
- with legacy timer compat, the corrected baseline-shaped callback family is enough to recover the scan progression branch
- with the current timer substrate, the same corrected callback family still fails because timer runtime dispatch never starts in the failing window

## What This Closes

Closed:
- the earlier `ieee80211_timer_process + 0xa0` pair-retarget result as evidence about callback-family impossibility
- slot callback family as the primary surviving cause of the current-timer failure by itself

Still live:
- why the current timer runtime does not start dispatching due timers in the failing branch
- what prevents `timer_runtime_diag entry_count` / `loop_count` / `selected_count` from becoming nonzero before `rust_scan_err`

## Exact Next Step

Instrument the current timer-substrate runtime entry path.

Highest-value target:
- the current timer task/bootstrap/dispatch path before the first timer callback execution
- specifically why the current runtime never reaches:
  - `entry_count > 0`
  - `loop_count > 0`
  - `selected_count > 0`

The best current model is:
- the empty-result bug is no longer primarily explained by slot callback family choice
- the surviving boundary is the current timer runtime dispatch path itself
