# 2026-03-17 Wi-Fi History-Narrowing Follow-Up

## Scope

This follow-up continues the investigation from
`docs/development/2026-03-17-wifi-history-narrowing-note.md` after the outer
runtime/global probes stopped producing discriminating signals.

## New Boundary

The app/comparator split is still:

- app path: `scan_rc=0`, `ScanDone`, empty list before retrieval, `ap_num=0`
- comparator: `scan_rc=0`, populated list before retrieval, nonzero `ap_num`

But the latest low-perturbation probes now show the following all match or are
non-causal:

1. `g_ic+0x1f0` scratch bytes
2. `g_ic+0x200` parse/cipher scratch bytes
3. later `g_wifi_nvs` gate bytes (`0x361`, `0x364`, `0x415`, `0x417`, `0x418`)
4. corrected `g_misc_nvs` target contents
5. init-stage Wi-Fi process setup and queue/process counts
6. `show_hidden`
7. the first post-`scan_check_hidden` reject block

Artifacts for these closures:

- `logs/flash_capture_20260317_ic_scratch_app/capture.log`
- `logs/flash_capture_20260317_ic_scratch_comparator/capture.log`
- `logs/flash_capture_20260317_ic_parse_app/capture.log`
- `logs/flash_capture_20260317_ic_parse_comparator/capture.log`
- `logs/flash_capture_20260317_show_hidden_true_app/capture.log`
- `logs/flash_capture_20260317_show_hidden_false_app/capture.log`
- `logs/flash_capture_20260317_init_trace_app/capture.log`
- `logs/flash_capture_20260317_process_counts_app/capture.log`

## Important New Finding

The `g_ic+0x1b4` target is not the transient candidate/result object we hoped it
was.

Evidence:

- app `g_ic+0x1b4` target bytes at `0x34..0x3c` decode to function-pointer-like
  values:
  - `0x40154424 -> wpa_ap_get_peer_spp_msg`
  - `0x40148944 -> wpa_config_parse_string`
- comparator `g_ic+0x1b4` target bytes at the same offsets decode to the same
  logical functions in its own image:
  - `0x40125820 -> wpa_ap_get_peer_spp_msg`
  - `0x4011a728 -> wpa_config_parse_string`

That means the visible differences at `0x34..0x3c` are just image-local pointer
relocations, not a useful state split.

Supporting artifacts:

- `logs/flash_capture_20260317_ic_1b4_fields_app/capture.log`
- `logs/flash_capture_20260317_ic_1b4_fields_comparator/capture.log`

## 2026-03-18 Boundary Update

Status: active

The app and comparator still match at these scan-start layers:

1. `wifi_scan_start_process`
2. `ieee80211_sta_scan`
3. `scan_set_scan_id` / `scan_get_scan_id`

The first meaningful code-generation split is now lower, at `cnx_sta_scan_cmd`.

Evidence:

- app:
  - `target/xtensa-esp32-none-elf/debug/meditamer`
  - `cnx_sta_scan_cmd = 0x40131ab8`, size `0x2f4`
- comparator:
  - `tools/esp_wifi_legacy_nostd_control/target/xtensa-esp32-none-elf/debug/esp_wifi_legacy_nostd_control`
  - `cnx_sta_scan_cmd = 0x4010801c`, size `0x282`

The explicit-config branch is materially different even though both end by
calling `scan_start`.

## What Is Still Live

The live target is now inside the transient blob-side parse/materialization path
that is no longer exposed through the outer globals we have been probing.

More concretely, the strongest remaining target is:

- after the shared post-`scan_check_hidden` / `memcmp` path
- before the shared list-link writer increments `scannum` and updates
  `g_ic+0x130/+0x134`
- likely in transient `scan_profile_check` / `scan_parse_beacon` object state
  rather than in a stable global

## Static Evidence Supporting This

The list-link writer remains structurally shared between app and comparator.
The latest static comparison instead points at deeper parse-stage divergence:

- shared writer-side structure still performs:
  - link via `g_ic+0x134`
  - increment `scannum`
  - post-link callback dispatch
- app/comparator still diverge in the broader parse generation around:
  - `scan_profile_check`
  - `scan_parse_beacon`
  - `ieee80211_parse_wpa`
  - `ieee80211_parse_rsn`
  - `ieee80211_parse_wapi`

## Next Phase

### Phase A: Stop Adding Outer Global Probes

Status: complete

Rationale:

- the accessible outer/global state is no longer separating app from comparator
- continuing to add more similar probes is low-yield

### Phase B: Move To Transient Parse-State Capture

Status: active

Objective:

Capture the transient object/branch state that exists between:

1. successful scan start
2. shared post-hidden checks
3. eventual list-link write

### Phase C: Accept More Invasive Instrumentation If Needed

Status: active

Allowed next steps:

1. blob-side or ABI-level probe aimed at the transient `a3` object used in
   `scan_profile_check`
2. targeted watch/probe of fields touched later in the parse path:
   - `+0x3c`
   - `+0x5d..0x5f`
   - `+0x7c`
   - `+0x88`
3. deeper static control-flow comparison after the shared post-hidden block,
   especially around WPA/RSN/WAPI parse handoff


## 2026-03-18 Direct `ieee80211_sta_scan` Trampoline

Status: Completed

- Replaced the app-side `wifi_scan_start_process` diagnostic wrapper with a direct call into `ieee80211_sta_scan`, preserving the visible guard logic and argument shaping already confirmed from disassembly.
- Result did not move the boundary. The app still reaches `scan_rc=0`, `ScanDone`, empty pre-retrieval list, and `ap_num=0`.
- Artifact: `logs/flash_capture_20260318_direct_sta_shim_app/capture.log`

Key evidence:
- `scan_process_wrap_obj ... fn=wifi_scan_start_process` still shows the same queue item and forwarded config block as previous runs.
- `idf_explicit_compare_postcall=postcall scan_rc=0 ... scan_done_count=1 scan_done_ap_num=0`
- `scan_list_probe ... phase=before_get_ap_num scannum=0x0000 head_ptr=0x0`
- `idf_explicit_compare=ok ... ap_num=0 records_returned=0`

Interpretation:
- The live boundary is now confirmed below `wifi_scan_start_process` and below the accepted command handoff into `ieee80211_sta_scan`.
- The next useful target is the non-interposable continuation inside `ieee80211_sta_scan` / `cnx_sta_scan_cmd`, not another outer command-object probe.

## Stop Conditions

Stop this line and regroup if any next probe:

1. pushes the app back into the earlier `scan_rc=12300` admission-failure form
2. only re-proves a closed outer-global hypothesis
3. cannot observe transient state more directly than the already-matched probes

## 2026-03-18 `scan_build_chan_list` Runtime Confirmation

Status: active

Fresh helper-buffer captures moved the boundary inside `cnx_sta_scan_cmd`
channel-plan preparation.

Artifacts:

- app explicit-first: `logs/flash_capture_20260318_helper_wrap_app_explicit_buffers/capture.log`
- comparator explicit compare: `logs/flash_capture_20260318_helper_wrap_comparator_buffers/capture.log`

Key result:

- app explicit compare reaches `scan_build_chan_list` with `arg2=0x3ffbe8cc`
  and `arg3=0x3ffc960c`, but the wrapped `arg2` bytes stay
  `00:00:00:00:00:00:00:00` before and after the call
- comparator explicit compare reaches `scan_build_chan_list` with
  `arg2=0x3ffc5f40` and `arg3=0x3ffc58b0`, and the wrapped `arg2` bytes are
  already non-zero before the call and remain
  `01:00:01:00:01:00:6c:09`
- app then stays at `scannum=0`, `head_ptr=0`, `ap_num=0`
- comparator materializes a linked list and returns non-zero `ap_num`

Interpretation:

- the live split is no longer just helper call order
- the stronger discriminator is the channel-plan state handed into or produced
  around `scan_build_chan_list`
- the next useful target is the origin and ownership of that `arg2` buffer in
  app vs comparator, not another outer scan wrapper seam


## Runtime Confirmation At `wifi_scan_start_process`
Status: complete
Artifacts: `logs/flash_capture_20260318_start_process_obj_app/capture.log`, `logs/flash_capture_20260318_start_process_obj_comparator_fixed/capture.log`
What this proved:
- the widened `--wrap=wifi_scan_start_process` seam is usable on both images
- the transient object entering `wifi_scan_start_process` matches across app and comparator on all currently probed non-pointer fields, and the forwarded config block at `queue_item+20` also matches on the fields that `cnx_sta_scan_cmd` actually reads: `w0=0`, `w4=0`, `b8=0`, `b9=1`, `w12=0`, `w16=0x0000000a`, `w20=0x00000014`, `w24=0`, `b28=0`, `h32=0`, `w36=0`; the only word-sized mismatch left at `w40` is in high bytes, while the live low byte consumed by `l8ui +40` is still `0`
- `word4` is an instruction-space pointer, not a readable data object; treating it as data caused `LoadStoreError` on both images, so it is not a useful nested-data discriminator
- the only nested-data difference we recovered is the RAM object behind `word16`: app `ptr16=[0x00000100,0x00000020,0x00000000,0x00000001]`; comparator `ptr16=[0x00000000,0x3ffb6240,0x0000ff18,0x00000000]`
- that `word16` nested object lines up with each image's `thread_sem_ptr`, so it is almost certainly per-thread semaphore context rather than scan-control state
- the runtime split therefore survives a matched `wifi_scan_start_process`
  command object: app `scan_rc=0`, `scannum=0`, `head_ptr=0`, `ap_num=0`;
  comparator `scan_rc=0`, nonzero `scannum`, non-null `head_ptr`, `ap_num=6`
- static disassembly narrows the seam further: `wifi_scan_start_process` only
  uses `byte8`, `word12`, and the parameter block starting at `a2+20`, so the
  observed `word16` nested-object difference is real but not yet causal at this
  call boundary
- pre-call and post-call snapshots are identical on both images, so the divergence does not begin as an in-place mutation at this seam or in the forwarded config block
- lower wrapped functions still do not fire through this accepted path, which keeps the live target in the non-interposable continuation below `wifi_scan_start_process`

Updated implication:
- the active boundary is now below the `wifi_scan_start_process` input object
- the next high-value runtime seam is the first producer/dispatcher that should
  feed beacon work into the parse/materialization path after that command is
  accepted
