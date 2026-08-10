# 2026-03-18 Wi-Fi History Narrowing Follow-up 07

## Current Boundary

The first interposable start-path seam is now confirmed on both images:

- app image: `chm_init`
- working comparator image: `chm_init`

This seam is earlier than the failing explicit-scan result window and earlier than result-list materialization.

## Runtime Confirmation

App capture:
- `logs/flash_capture_20260318_start_path_wrap_app_full_setfn/capture.log`

Comparator captures:
- `logs/flash_capture_20260318_start_path_wrap_comparator/capture.log`
- `logs/flash_capture_20260318_start_path_wrap_comparator_afterstart_timer/capture.log`

At the start seam:

- app `chm_init`:
  - `pre_timer_setfn=0`
  - `post_timer_setfn=2`
  - `pre_timer_arm=0`
  - `post_timer_arm=0`
  - `pre_op_chan=0x00`
  - `post_op_chan=0xff`
- comparator `chm_init`:
  - `pre_timer_setfn=12`
  - `post_timer_setfn=14`
  - `pre_timer_arm=10`
  - `post_timer_arm=11`
  - `pre_op_chan=0x00`
  - `post_op_chan=0xff`

So both paths reach the same start seam and both flip `op_chan` to `0xff`, but the timer-family work emitted there differs.

## Exact Timer Callback Split

App `after_start_pre_driver_state` timer registrations:
- callback `0x40120888` -> `nan_dp_schedule_ndc_start`
- callback `0x40120888` -> `nan_dp_schedule_ndc_start` with `arg=1`
- callback `0x40126f3c` -> `ieee80211_rfid_locp_recv`

Comparator `after_start` timer registrations:
- callback `0x401094bc` -> `ieee80211_timer_process`
- callback `0x401094bc` -> `ieee80211_timer_process` with `arg=1`
- later setfn in the same window includes `eloop_run_timer`, but that is not part of the `chm_init` delta itself

This is the strongest live discriminator so far.

## Static Confirmation

App `chm_init` directly installs `nan_dp_schedule_ndc_start`:
- `target/xtensa-esp32-none-elf/debug/meditamer`
- `chm_init` at `0x40120d5c`
- setfn site loads `0x40120888 <nan_dp_schedule_ndc_start+0x68>`

Comparator `chm_init` directly installs `ieee80211_timer_process`:
- `tools/esp_wifi_legacy_nostd_control/target/xtensa-esp32-none-elf/debug/esp_wifi_legacy_nostd_control`
- `chm_init` at `0x40109884`
- setfn site loads `0x401094bc <ieee80211_timer_process+0xa0>`

So this is not just a later runtime correlation. The callback substitution exists directly in `chm_init` itself.

## Relation To Older History

This is consistent with older retained history in:
- `docs/development/wifi-upload-decision-ledger.md`
- `docs/development/upload-throughput-history/part-16.md`

Older history already established:
- `chm_init` registers the dominant `nan_dp_schedule_ndc_start` family in the failing app path
- suppressing the `arg=1` branch materially changes the failure shape
- `chm_init` is the direct registration and arm site for that timer family in the current app path

What is new here is the same-seam comparison against the working comparator:
- the comparator does not install the NAN callback family there
- it installs `ieee80211_timer_process` instead

## Current Interpretation

The live boundary has tightened from generic start-path suspicion to a direct callback-family substitution inside `chm_init`:

- app start path: `chm_init -> nan_dp_schedule_ndc_start`
- comparator start path: `chm_init -> ieee80211_timer_process`

This is now the strongest root-cause candidate.

## Next Step

Run one diagnostics-only causality test:

- wrap `nan_dp_schedule_ndc_start` in the app image
- redirect it to `ieee80211_timer_process`
- keep the rest of the app path unchanged

Interpretation:
- if AP results appear, the callback substitution in `chm_init` is likely the decisive cause
- if the failure shape changes materially but does not recover, the substitution is still causal but not sufficient alone
- if nothing changes, the callback substitution is correlated but not sufficient by itself
