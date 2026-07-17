# 2026-03-18 Wi-Fi History Narrowing Follow-up 10

## Scope

This follow-up records the timer-execution-ring reruns for the exact-slot `chm_init` retarget matrix and closes the ambiguity in the paired-slot case.

The earlier long-window capture for the paired retarget stopped near `idf_explicit_compare_prestart`. A rerun with a larger boot window shows that the paired retarget does reach `idf_explicit_compare_postcall` and deterministically flips the app into the earlier `scan_rc=12300` failure form.

## Matrix

Captures:
- baseline: `logs/flash_capture_20260318_timer_exec_ring_baseline_long_app/capture.log`
- `arg=0` only: `logs/flash_capture_20260318_timer_exec_ring_arg0_long_app/capture.log`
- `arg=1` only: `logs/flash_capture_20260318_timer_exec_ring_arg1_long_app/capture.log`
- paired retarget, first long run: `logs/flash_capture_20260318_timer_exec_ring_both_long_app/capture.log`
- paired retarget, confirmation rerun: `logs/flash_capture_20260318_131834/capture.log`

## Stable Single-Slot Result

The single-slot result is now stable across the execution-ring instrumentation:

- baseline remains in the `scan_rc=0`, `ScanDone`, empty-list branch
- `arg=0` retarget only remains in the same branch
- `arg=1` retarget only remains in the same branch

The new execution ring makes the mixed execution pattern concrete:

- baseline executes only the original `arg=0` callback family at `10000 us`
- `arg=1` only still executes only the original `arg=0` callback family at `10000 us`
- `arg=0` only alternates between:
  - retargeted `arg=0` callback at `10000 us`
  - original `arg=1` callback at `20000 us`

But none of those single-slot states restore AP results.

## Paired-Retarget Confirmation

Confirmation capture:
- `logs/flash_capture_20260318_131834/capture.log`

Key postcall lines:
- `idf_explicit_compare_postcall=postcall scan_rc=12300`
- `scan_done_count=0`
- `blob_chm after=idf_explicit_compare_postcall op_chan=0x01 ptr_08=0xa ptr_0c=0x14 ptr_10=0x3ffc9f9c ptr_14=0x40123310`
- `blob_scan after=idf_explicit_compare_postcall word_00=0x0000010f word_30=0x00000014 word_34=0x0000000a byte_44=0x03 byte_45=0x01 byte_46=0x03 byte_47=0x01 flags_70=0x03 flags_71=0x01 word_114=0x00000000`

The paired retarget is therefore a real causal switch, not a logging artifact.

## Postcall Arm/Exec Result

The confirmation rerun also adds one important negative result.

At `idf_explicit_compare_postcall`, the paired-retarget log emits:
- no `timer_exec_diag after=idf_explicit_compare_postcall`
- no `timer_exec_recent after=idf_explicit_compare_postcall`
- no `timer_compat_arm_recent after=idf_explicit_compare_postcall`

So in this build shape, the paired retarget has already switched the app into the earlier `12300` branch before any retained postcall timer-execution evidence appears.

That makes callback identity and setup-state consumption more plausible than a simple "callbacks executed later and caused the branch" explanation.

## Callback Address Drift

Because the diagnostic build shape changed, the exact callback addresses moved again.

Current paired-retarget rerun:
- original duplicated callback family from `chm_init`: `0x401217ec`
- retarget callback family: `0x40132630`

This does not change the interpretation. These are internal thunk addresses in the current diagnostic image, not stable public symbol entries.

## Stronger Interpretation

The joint `chm_init` timer-slot pair is now constrained more tightly:

- the pair is causally upstream of the branch split
- neither slot alone is sufficient
- the paired retarget does not recover the comparator path
- instead, the paired retarget drives the app into the earlier admission/state failure form before any `ScanDone`

That means the timer-slot pair is part of the control path that chooses between:
- the app's baseline `scan_rc=0` but empty-list branch
- the earlier `scan_rc=12300` branch

It is not sufficient to produce the working comparator path by itself.

## Best Next Step

Instrument the first consumer that observes the `chm_init` timer-slot pair before explicit scan admission.

Practical target:
1. compare baseline app vs paired retarget at the earliest arm/dispatch seam after `after_start_ok`
2. determine whether the scan/channel-manager path consults callback identity before callback execution
3. if so, move one step earlier and treat timer-callback identity as setup-state consumed by the scan/channel-manager path rather than as ordinary deferred work
