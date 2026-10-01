# 2026-03-18 Wi-Fi History-Narrowing Follow-Up 03

## Scope

This follow-up validates whether the earlier `scan_build_chan_list` buffer split
was real or an artifact of the app-side `wifi_scan_start_process` wrapper.

Primary predecessors:

- `docs/development/2026-03-18-wifi-history-narrowing-followup-02.md`
- `docs/development/2026-03-17-wifi-history-narrowing-followup.md`

## What Changed

The app-side `__wrap_wifi_scan_start_process` used by the runtime-confirmation
build was not a passive wrapper. It reimplemented the scan-start path in Rust,
including a direct call into `cnx_sta_scan_cmd`, while the comparator wrapper
was a true pass-through to `__real_wifi_scan_start_process`.

That made the previous deep helper-buffer result potentially suspect until it
was rechecked under a passive app wrapper.

## Passive Recheck

Artifacts:

- app, passive `wifi_scan_start_process` wrapper:
  - `logs/flash_capture_20260318_helper_wrap_app_explicit_buffers_passive_wrap/capture.log`
- comparator control:
  - `logs/flash_capture_20260318_helper_wrap_comparator_buffers/capture.log`

## Result

The app still reproduces the same failure family under the passive wrapper:

- `idf_explicit_compare=ok`
- `scan_rc=0`
- `scannum=0x0000`
- `head_ptr=0x0`
- `ap_num=0`

The key helper split also survives unchanged:

- app `scan_build_chan_list`
  - `arg2=0x3ffbe8cc`
  - `arg3=0x3ffc960c`
  - `pre_arg2=00:00:00:00:00:00:00:00`
  - `post_arg2=00:00:00:00:00:00:00:00`
- comparator `scan_build_chan_list`
  - `arg2=0x3ffc5f40`
  - `arg3=0x3ffc58b0`
  - `pre_arg2=01:00:01:00:01:00:6c:09`
  - `post_arg2=01:00:01:00:01:00:6c:09`

The app pointer identities at the same checkpoint remain:

- `sta_ptr=0x3ffbe8cc`
- `chm_ptr=0x3ffc90c0`
- `g_wifi_nvs_ptr=0x3ffc960c`

So the passive rerun preserves the same relation seen in Follow-Up 02:

- app `scan_build_chan_list arg2 == sta_ptr`
- app `scan_build_chan_list arg3 == g_wifi_nvs_ptr`
- comparator `scan_build_chan_list arg3 == g_wifi_nvs_ptr`
- comparator `scan_build_chan_list arg2 == chm_ptr + 0x50`

## Meaning

This closes the instrumentation-validity objection.

The `scan_build_chan_list` working-buffer split is not an artifact of the
invasive app `wifi_scan_start_process` wrapper. It survives when the app wrapper
is reduced to a true pass-through.

That makes the current discriminator substantially stronger:

1. the app is reaching the same helper
2. the app is still feeding a different effective working buffer into it
3. the app still never materializes the scan result list

## Strongest Current Hypothesis

The strongest live hypothesis is now:

- the app-side `cnx_sta_scan_cmd` continuation seeds or forwards the
  `scan_build_chan_list` working buffer incorrectly, causing channel-plan setup
  to operate on `sta_ptr` state rather than the comparator's channel-manager
  working buffer (`chm_ptr + 0x50`)

## Next Step

Highest-value next step:

1. add a tightly scoped runtime experiment that rewrites only
   `scan_build_chan_list arg2` on the app path to the comparator-style buffer
   (`chm_ptr + 0x50`) when the same pointer pattern is observed
2. rerun the same explicit-first capture
3. check whether that alone moves:
   - `scannum`
   - `head_ptr`
   - `ap_num`

If it does, the buffer-origin hypothesis becomes causal rather than just
correlative.

## Stop Conditions

Stop this line and regroup if the targeted arg rewrite:

1. changes the failure family to an earlier admission failure
2. crashes or corrupts unrelated runtime state
3. leaves `scan_build_chan_list arg2` changed but still produces the same empty
   list, which would push the target one step later than buffer origin
