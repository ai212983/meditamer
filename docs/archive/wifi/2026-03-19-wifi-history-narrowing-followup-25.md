# 2026-03-19 Wi-Fi History Narrowing Follow-up 25

## Scope

This follow-up corrects the previous current-timer interpretation from follow-up 24.

## Trigger

The current-substrate slot-retarget diagnostic in
`src/firmware/storage/upload/wifi/connect/nan_timer_slot_retarget_diag.rs`
was still targeting `ieee80211_timer_process + 0xa0`.

That target is not equivalent to the legacy trampoline used in the successful
legacy-compat recovery branch.

## Change

Added current-substrate trampoline support so the retarget path can use the same
logical callback body as the legacy success case:

- `ieee80211_timer_process_scan_step(7, 8, arg)`

The current retarget helper now chooses between:

- `ieee80211_timer_process + 0xa0`
- `ieee80211_timer_process_scan_step_trampoline`

under `MEDITAMER_WIFI_NAN_TIMER_SLOT_RETARGET_TRAMPOLINE_DIAG=1`.

## Artifact

- current substrate + corrected trampoline retarget:
  - [flash_capture_20260319_094108](../../logs/flash_capture_20260319_094108/capture.log)

## Proven Result

The corrected trampoline materially changes the current-substrate branch.

After retarget:

- `from_callback_ptr=0x401144b0`
- `to_callback_ptr=0x400d9af4`
- `matched_count=2`
- `retargeted_count=2`

At explicit-scan postcall:

- `scan_rc=0`
- `scan_done_count=1`
- `scan_done_status=0`
- `scan_done_ap_num=0`
- `blob_scan word_114=0x00000080`
- `blob_chm op_chan=0xff`

But the app still does not materialize AP results:

- `scan_list_snapshot ... scannum=0x0000`
- `head_ptr=0x00000000`
- `tail_ptr=0x3ffcb354`

## Interpretation

This retires the previous claim that the current timer runtime itself was the
surviving blocker.

The earlier current-substrate `12300` branch was contaminated by an incorrect
internal callback target.

With the corrected trampoline:

- modern timer wake/dispatch is no longer the active blocker
- the branch returns to the familiar app failure family
- the active boundary is again:
  - `scan_rc=0`
  - `ScanDone`
  - no linked scan-result list

So the timer-runtime branch is no longer the highest-value active hypothesis for
root cause.

## Current Boundary

Still live:

- scan completes
- `scan_id` progresses (`word_114=0x80`)
- `ScanDone` is observed
- result list is empty before retrieval

Closed by this iteration:

- incorrect claim that the current timer task/runtime could not reach recovery
- incorrect claim that current-substrate callback execution was the main
  remaining blocker

## Next Step

Return to the earlier stable boundary and compare current baseline app against
current-substrate corrected-trampoline retarget at the zero-result stage.

The next useful target is not timer dispatch. It is again the path between:

- successful scan completion
- and result-list materialization / list linking
