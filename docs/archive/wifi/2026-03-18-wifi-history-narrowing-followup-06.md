# 2026-03-18 Wi-Fi History Narrowing Follow-up 06

## Scope

Record the current timer-family runtime split, connect it back to prior history, and state the next non-redundant runtime step.

## Current Runtime Result

Current app diagnostic capture:
- [app setfn callers](../../logs/flash_capture_20260318_app_setfn_callers/capture.log)

Current comparator control capture:
- [comparator timer-wrap handle](../../logs/flash_capture_20260318_timer_wrap_comparator_handle/capture.log)

Observed app behavior in the failing explicit-scan window:
- `scan_rc=0`
- `ScanDone` fires
- result list is already empty before retrieval
- `ap_num=0`

Observed comparator behavior in the successful explicit-scan window:
- `scan_rc=0`
- list is populated before retrieval
- repeated timer arms target the `cnx_connect_timeout` callback family

## Timer-Family Split

From the app capture:
- `timer_compat_setfn_recent` shows three registered callbacks in the app image before or during the explicit compare window:
  - `0x401202d4 -> nan_dp_schedule_ndc_start`
  - `0x40126988 -> ieee80211_rfid_locp_recv`
  - `0x40132e08 -> cnx_connect_timeout`
- repeated `timer_compat_arm_recent` entries in the failing explicit-scan window target only:
  - `0x401202d4 -> nan_dp_schedule_ndc_start`
- the repeated arm callsites resolve to:
  - `0x401209a9 -> chm_init`
  - `0x401209d9 -> chm_init`

From the comparator capture:
- repeated timer arms in the successful explicit-scan window target:
  - `0x4010967c -> cnx_connect_timeout`

## What This Means

The current app failure is not well explained by missing timer registration alone.

The app does register `cnx_connect_timeout`, but in the observed failing explicit-scan window it repeatedly arms the `nan_dp_schedule_ndc_start` family from `chm_init` instead.

That tightens the active boundary to:
- state and preconditions entering `wifi_hw_start -> chm_init`
- or the channel-manager setup path that leaves the app in the NAN/channel-scheduler timer family instead of the comparator's `cnx_connect_timeout` path


## Additional Runtime Confirmation

From [after-set-mode capture](../../logs/flash_capture_20260318_after_set_mode/capture.log):
- `before_diag_reset`: timer registrations already exist, but they are not the later NAN/RFID/cnx-timeout scan-window callbacks; the visible callbacks resolve into early `wifi_log` / `chip_v7_set_chan_ana` families while Wi-Fi global state is still zeroed (`sta_ptr=0`, `current_chan=0`, `word_114=0`).
- `after_set_mode`: `wifi_set_mode` flips `wifi_nvs_byte_00` from `0` to `1`, but timer registrations are fully reset and still absent (`setfn_count=0`, `arm_count=0`).
- `after_start_pre_driver_state`: the NAN/RFID-family registrations first appear here, after `wifi_start_async` and before any timer arms (`setfn_count=3`, `arm_count=0`).

This closes another seam:
- the relevant timer-family registrations are not present before the harness reset
- they are not introduced by `wifi_set_mode`
- they first appear during `wifi_start_async`
- the repeated arm traffic only comes later, during the explicit-scan window

## History Gate

This timer-family branch is not novel.

The following exact diagnostic knobs were already exercised and are recorded as completed in the decision ledger and history:
- `MEDITAMER_WIFI_SUPPRESS_NAN_DP_TIMER_ARM_DIAG`
- `MEDITAMER_WIFI_SUPPRESS_NAN_DP_TIMER_ARG1_ARM_DIAG`
- `MEDITAMER_WIFI_SUPPRESS_NAN_DP_TIMER_ARG0_ARM_DIAG`
- `MEDITAMER_WIFI_SUPPRESS_NAN_DP_TIMER_ARG1_SETFN_DIAG`

Relevant records:
- [decision ledger](./wifi-upload-decision-ledger.md)
- [history part-16](./upload-throughput-history/part-16.md)

Those runs already established:
- the `nan_dp_schedule_ndc_start` timer family is causally involved in the failing shape
- suppressing it materially changes the failure mode
- those knobs are diagnostic-only and not a production fix

Per the novelty gate, they should not be rerun unless explicit reconfirmation is requested.

## Next Step

The low-perturbation start-boundary probe is now closed:
- the relevant NAN/RFID timer registrations are not present before the diagnostic reset
- they are not introduced by `wifi_set_mode`
- they first appear during `wifi_start_async`
- they are still only registrations at `after_start_pre_driver_state`; the repeated NAN-family arm traffic comes later in the explicit-scan window

So the next non-redundant step is no longer another outer-state dump.

It is an invasive probe inside the Wi‑Fi start continuation, specifically between:
- `wifi_start_async`
- the first post-start timer registrations
- and the later explicit-scan window where `chm_init` repeatedly arms the NAN timer family

Practically, that means instrumenting inside the app's start path rather than around it:
- `_do_wifi_start`
- `wifi_hw_start`
- `chm_init`

The active question is now:
- why the app start path registers the NAN/RFID-family timers during `wifi_start_async`, while the successful comparator later progresses into the `cnx_connect_timeout`-dominated scan path that materializes AP results
