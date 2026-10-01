# 2026-03-18 Wi-Fi History-Narrowing Follow-Up 02

## Scope

This follow-up records the first runtime-confirmed discriminator inside
`cnx_sta_scan_cmd` channel-plan preparation after the earlier outer scan-start
seams were closed.

Primary predecessors:

- `docs/development/2026-03-17-wifi-history-narrowing-note.md`
- `docs/development/2026-03-17-wifi-history-narrowing-followup.md`

## New Runtime Result

Status: active

Fresh helper-buffer captures show that app and comparator no longer just differ
in helper call order. They differ in the effective working buffer state passed
into `scan_build_chan_list` during explicit broad scan setup.

Artifacts:

- app explicit-first:
  - `logs/flash_capture_20260318_helper_wrap_app_explicit_buffers/capture.log`
- comparator explicit compare:
  - `logs/flash_capture_20260318_helper_wrap_comparator_buffers/capture.log`

## App Path

Relevant runtime lines:

- `blob_state after=idf_explicit_compare_postcall`
  - `sta_ptr=0x3ffbe8cc`
  - `chm_ptr=0x3ffc90c0`
  - `g_wifi_nvs_ptr=0x3ffc960c`
- `scan_cmd_helper_wrap_diag_entry after=idf_explicit_compare_postcall idx=3 fn=scan_build_chan_list`
  - `arg2=0x3ffbe8cc`
  - `arg3=0x3ffc960c`
  - `pre_arg2=00:00:00:00:00:00:00:00`
  - `post_arg2=00:00:00:00:00:00:00:00`
  - `pre_arg3=01:00:00:00:ff:ff:ff:ff`
  - `post_arg3=01:00:00:00:ff:ff:ff:ff`
- `scan_list_probe label=idf_explicit_compare phase=before_get_ap_num`
  - `scannum=0x0000`
  - `head_ptr=0x0`
- `idf_explicit_compare=ok`
  - `ap_num=0`
  - `records_returned=0`

Meaning:

- app reaches `scan_build_chan_list`
- the wrapped `arg2` buffer is still zero before and after the helper call
- the app still ends with an empty result list and zero APs

## Comparator Path

Relevant runtime lines:

- `scan_cmd_helper_wrap_diag_entry label=idf_explicit_compare phase=before_get_ap_num idx=4 fn=scan_build_chan_list`
  - `arg2=0x3ffc5f40`
  - `arg3=0x3ffc58b0`
  - `pre_arg2=01:00:01:00:01:00:6c:09`
  - `post_arg2=01:00:01:00:01:00:6c:09`
  - `pre_arg3=01:00:00:00:ff:ff:ff:ff`
  - `post_arg3=01:00:00:00:ff:ff:ff:ff`
- `scan_list_probe label=idf_explicit_compare phase=before_get_ap_num`
  - `scannum=0x0005`
  - `head_ptr=0x3ffbcd0c`
- `idf_explicit_compare=ok`
  - `ap_num=5`
  - `records_returned=5`

Meaning:

- comparator reaches the same helper with a materially different `arg2` target
- the wrapped `arg2` bytes are already non-zero and remain stable
- the comparator materializes a linked result list and returns APs

## Static Call-Site Comparison

Disassembly confirms the split is at the `cnx_sta_scan_cmd -> scan_build_chan_list`
call setup.

Artifacts:

- app image:
  - `target/xtensa-esp32-none-elf/debug/meditamer`
- comparator image:
  - `tools/esp_wifi_legacy_nostd_control/target/xtensa-esp32-none-elf/debug/esp_wifi_legacy_nostd_control`

Resolved symbols:

- app:
  - `scan_build_chan_list = 0x40122084`
  - `cnx_sta_scan_cmd = 0x40132e08`
- comparator:
  - `scan_build_chan_list = 0x401020c8`
  - `cnx_sta_scan_cmd = 0x40109330`

Call-site slices:

- app `cnx_sta_scan_cmd+0x2d4..0x2df`
  - `movi a10, 1`
  - `or a11, a2, a2`
  - `slli a10, a10, 17`
  - `call8 __wrap_scan_build_chan_list`
- comparator `cnx_sta_scan_cmd+0x269..0x26f`
  - `l32r a10, 0x20000`
  - `mov.n a11, a2`
  - `call8 __wrap_scan_build_chan_list`

Interpretation:

- both images still call `scan_build_chan_list` from `cnx_sta_scan_cmd`
- the surrounding setup is not identical
- the runtime-wrapped arguments and the call-site structure together support a
  real channel-plan preparation split below the already-closed outer scan-start
  seams

## Strongest Current Hypothesis

The active discriminator is now one of:

1. app passes the wrong working buffer into `scan_build_chan_list`
2. app passes the right logical object, but its target buffer is never seeded
3. app and comparator interpret the same helper argument differently because the
   surrounding `cnx_sta_scan_cmd` setup diverges before or after the helper

The first option is currently the strongest live hypothesis because:

- app helper `arg2` equals app `sta_ptr`
- comparator helper `arg2` does not equal comparator `sta_ptr`
- comparator helper `arg2` is non-zero while app helper `arg2` stays zero

## Next Step

Highest-value next step:

1. trace the exact origin of the helper working-buffer argument at the
   `cnx_sta_scan_cmd -> scan_build_chan_list` call site in both images
2. prove whether the comparator is routing channel-plan state through
   `chm_ptr + 0x50` or another derived buffer while the app routes it through
   `sta_ptr`
3. if confirmed, instrument or patch that exact setup point rather than adding
   more outer scan wrappers

## Stop Conditions

Stop this line and regroup if a next probe:

1. only re-proves helper call order without clarifying buffer origin
2. pushes the app back into the earlier `scan_rc=12300` admission-failure form
3. widens scope back to already-closed outer scan-start seams
