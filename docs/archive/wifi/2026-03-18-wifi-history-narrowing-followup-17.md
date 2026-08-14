# 2026-03-18 Wi-Fi History Narrowing Follow-up 17

## Scope

This follow-up records the first attempt to combine both surviving factors:

- old timer substrate semantics
- old `g_chm` slot callback family

## Experiments

### 1. Unpatched combined run

Configuration:

- `MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_TIMER_COMPAT_DIAG=1`
- `MEDITAMER_WIFI_NAN_TIMER_SLOT_RETARGET_DIAG=1`
- boot-scan explicit compare enabled

Artifact:

- `logs/flash_capture_20260318_legacytimercompat_plus_slotretarget/capture.log`

Result:

- `scan_rc=0`
- `ScanDone`
- `scannum=0`
- `ap_num=0`
- `matched_count=0`
- `retargeted_count=0`

Interpretation:

- the old-substrate branch prevents the existing slot-retarget hook from matching anything useful
- the runtime retarget path used on the new substrate is not sufficient to exercise the combined case here

### 2. Patched legacy-timer `compat_timer_setfn` retarget attempt

Change:

- legacy `compat_timer_setfn` now tries to rewrite
  - `nan_dp_schedule_ndc_start + 0x68`
  - to `ieee80211_timer_process + 0xa0`
  - for `arg=0/1`

Artifact:

- `logs/flash_capture_20260318_legacytimercompat_plus_slotretarget_patched/capture.log`

Observed result:

- still `scan_rc=0`
- still empty `scan_done_list`
- still `ap_num=0`
- runtime stays on the stable zero-result branch:
  - `blob_chm op_chan=0xff`
  - `blob_scan word_00=0x00000000 word_30=0x00000078 word_114=0x00000080`

Important detail:

- the legacy timer summary after postcall still reports:
  - `last_callback_ptr=0x40122588`
- app symbol resolution shows this is still inside:
  - `nan_dp_schedule_ndc_start`

So the patched attempt did not actually prove that the old-callback family was installed under the old substrate.

## Conclusion

The combined hypothesis remains live, but it has not yet been exercised cleanly.

What is now proven:

- old timer substrate alone is not sufficient in full firmware
- new-substrate callback retarget alone is not sufficient
- the first old-substrate + callback-retarget attempts did not yet produce a clean installation of the old callback family in the legacy timer path

## Narrowed Next Step

Do not rerun more top-level boot-scan permutations.

The next useful step is to instrument the legacy timer-compat `compat_timer_setfn` path directly so it exposes a recent setfn ring like the new substrate diagnostics.

That should answer one precise question:

- under the legacy timer-compat branch, which callback family is actually being installed into the two `g_chm` slot timers during full firmware startup?
