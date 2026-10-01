# 2026-03-18 Wi-Fi History Narrowing Follow-up 08

## Redirect Test

Diagnostics-only app capture:
- `logs/flash_capture_20260318_nan_to_timer_redirect_app/capture.log`

Enabled knob:
- `MEDITAMER_WIFI_NAN_TO_TIMER_PROCESS_REDIRECT_DIAG=1`

## Result

The redirect wrapper did not fire:

- `nan_timer_redirect_diag after=after_start_pre_driver_state enabled=true redirect_count=0 passthrough_count=0`
- `nan_timer_redirect_diag after=idf_explicit_compare_postcall enabled=true redirect_count=0 passthrough_count=0`

The failure shape stayed unchanged:

- `scan_rc=0`
- `ScanDone status=0 count=0`
- `scannum=0x0000`
- `head_ptr=0`
- `ap_num=0`

## Why The Wrapper Missed

Static and runtime evidence now agree that `chm_init` does not install the symbol entry for `nan_dp_schedule_ndc_start`. It installs an internal offset inside that function:

- app `chm_init` loads `0x40120888 <nan_dp_schedule_ndc_start+0x68>`
- comparator `chm_init` loads `0x401094bc <ieee80211_timer_process+0xa0>`

That means a normal linker `--wrap=nan_dp_schedule_ndc_start` interposes only the symbol entry, not the literal callback pointer actually written by `chm_init`.

## What This Proves

This closes the cheap interposition branch.

- the callback-family substitution inside `chm_init` is still the strongest live discriminator
- but changing the symbol entry is not enough, because the blob uses a direct internal offset pointer

## Current Boundary

The live boundary is now:

- app `chm_init` writes `nan_dp_schedule_ndc_start+0x68` into the timer callback slots
- comparator `chm_init` writes `ieee80211_timer_process+0xa0` into the same logical slots
- the failing app path never materializes AP results after that start-path split

## Next Step

The next meaningful causality test has to be more invasive than symbol wrapping:

1. patch the exact callback literal written by app `chm_init`
2. or patch the `chm_init` setfn callsite itself
3. or patch the timer callback slot contents after registration and before the explicit scan window

A normal symbol-entry wrap is no longer sufficient for this branch.
