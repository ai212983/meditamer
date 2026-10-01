# 2026-03-18 Wi-Fi History Narrowing Follow-up 09

## Scope

This follow-up records the first successful exact-slot retarget of the two `chm_init` timer callbacks in the app image.

Unlike the earlier symbol-wrap attempt, this retarget operates on the actual timer slots recorded in `timer_compat_diag().recent_setfn_*` after `chm_init` has already registered them.

## Implementation

New diagnostics-only helper path:
- `vendor/esp-radio-0.17.0/src/compat/timer_compat.rs`
- `vendor/esp-radio-0.17.0/src/lib.rs`
- `src/firmware/storage/upload/wifi/connect/nan_timer_slot_retarget_diag.rs`
- `src/firmware/storage/upload/wifi/connect/boot_scan_diag/mod.rs`

Behavior:
1. Wait for `after_start_pre_driver_state`, where the app has already registered the two `chm_init` timers.
2. Inspect the recent `setfn` ring.
3. Infer the duplicated callback with `arg=0` and `arg=1`.
4. Retarget matching timer slots from the app callback family to the app image's internal `ieee80211_timer_process + 0xa0` callback literal.

## Exact-Slot Retarget: Both Slots

Capture:
- `logs/flash_capture_20260318_nan_timer_slot_retarget_app_inferred_long/capture.log`

Key proof that the helper hit the exact slots:
- `nan_timer_slot_retarget_diag after=after_nan_timer_slot_retarget ... matched_count=2 retargeted_count=2`
- `timer_compat_setfn_recent after=after_nan_timer_slot_retarget idx=4 ... callback_ptr=0x40131c68 arg_ptr=0x0`
- `timer_compat_setfn_recent after=after_nan_timer_slot_retarget idx=5 ... callback_ptr=0x40131c68 arg_ptr=0x1`

Effect on failure shape:
- `idf_explicit_compare_postcall=postcall scan_rc=12300`
- `scan_done_count=0`
- `blob_scan after=idf_explicit_compare_postcall word_00=0x0000010f`
- `blob_chm after=idf_explicit_compare_postcall op_chan=0x01 ptr_08=0x0a ptr_0c=0x14`

Interpretation:
- exact-slot callback substitution is causally live
- retargeting both `chm_init` timer slots does not restore results
- instead, it pushes the app into the earlier scan-start-admission failure form (`scan_rc=12300`)

## Arg-Split Matrix

### `arg=1` only

Capture:
- `logs/flash_capture_20260318_nan_timer_slot_retarget_arg1_app/capture.log`

Observed retarget:
- `matched_count=1 retargeted_count=1 last_arg_ptr=0x1`

Failure shape:
- `idf_explicit_compare_postcall=postcall scan_rc=0`
- `event scan_done_list ... scannum=0x0000 head_ptr=0x0`
- `idf_explicit_compare=ok scan_rc=0 ap_num=0 records_returned=0`

Conclusion:
- retargeting only the `arg=1` slot is not sufficient to move the app out of the baseline failure family

### `arg=0` only

Capture:
- `logs/flash_capture_20260318_nan_timer_slot_retarget_arg0_app/capture.log`

Observed retarget:
- `matched_count=1 retargeted_count=1 last_arg_ptr=0x0`

Failure shape:
- `idf_explicit_compare_postcall=postcall scan_rc=0`
- `event scan_done_list ... scannum=0x0000 head_ptr=0x0`
- `idf_explicit_compare=ok scan_rc=0 ap_num=0 records_returned=0`

Conclusion:
- retargeting only the `arg=0` slot is also not sufficient to move the app out of the baseline failure family

## Strongest Current Interpretation

The timer-slot branch is now constrained much more tightly:

- the exact `chm_init` timer-slot pair is causal
- neither single slot is sufficient by itself
- the combined pair changes the failure mode materially
- but the combined pair alone is still not sufficient to reproduce the working comparator path

So the live statement is now:

- the app/comparator split is not just correlated timer registration noise
- the paired `chm_init` callback family participates in the failure
- but successful comparator behavior requires additional coupled state beyond swapping those two callbacks in isolation

## Best Next Step

Do not spend more time on symbol-entry wraps.

The next highest-value step is to compare the immediate post-`chm_init` timer-consumer path under the exact-slot matrix:

1. baseline app
2. `arg=1` retarget only
3. `arg=0` retarget only
4. both retargeted

Specifically:
- which timer callback actually executes first
- whether the callback pair changes later queue traffic or channel-manager state before `scan_start`
- whether the joint retarget creates the `12300` state by changing follow-on channel-manager scheduling, not by result-list materialization directly
