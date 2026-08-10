# 2026-03-19 Wi-Fi History Narrowing Follow-up 26

## Scope

This follow-up compares the older current-timer retarget artifact against the
corrected current-timer trampoline retarget artifact.

## Artifacts

- earlier current-timer retarget artifact:
  - [flash_capture_20260319_currenttimer_pair_retarget_trampoline](../../logs/flash_capture_20260319_currenttimer_pair_retarget_trampoline/capture.log)
- corrected current-timer trampoline retarget artifact:
  - [flash_capture_20260319_094108](../../logs/flash_capture_20260319_094108/capture.log)

## Direct Comparison

Earlier artifact:

- `from_callback_ptr=0x4012392c`
- `to_callback_ptr=0x40134770`
- `matched_count=2`
- `retargeted_count=2`
- post-retarget `word_114=0x00000000`
- explicit compare failed in the earlier branch

Corrected artifact:

- `from_callback_ptr=0x401144b0`
- `to_callback_ptr=0x400d9af4`
- `matched_count=2`
- `retargeted_count=2`
- post-retarget `word_114=0x00000000`
- explicit compare postcall becomes:
  - `scan_rc=0`
  - `scan_done_count=1`
  - `scan_done_ap_num=0`
  - `blob_scan word_114=0x00000080`
  - `blob_chm op_chan=0xff`

## Result

The previous current-timer retarget branch was not a faithful substitution of
legacy callback behavior.

The corrected trampoline removes the earlier `12300` branch and returns the
system to the stable app failure family:

- scan completes
- `scan_id` progresses
- `ScanDone` is observed
- the result list is still empty before retrieval

## Interpretation

This means the timer-runtime branch no longer explains the root cause of the
stable app failure family.

What remains live is the older boundary already established in the scan-result
investigation:

- successful scan completion
- no scan-list materialization / linking

## Practical Consequence

Do not spend more time on outer timer-runtime wake/task hypotheses unless a new
signal reopens them.

The highest-value active work is again inside the result-list materialization
path, not the scheduler/timer runtime.
