# Wi-Fi History Narrowing Follow-up 27

Date: 2026-03-19

## Purpose

Record the corrected app-side wrap result after fixing the linker instrumentation gap around `scan_parse_beacon`, `scan_profile_check`, the `ieee80211_parse_*` helpers, and the `cnx_*` list materialization helpers.

## What Changed

The app `build.rs` previously wrapped several scan/timer symbols but did **not** add linker wraps for:

- `scan_parse_beacon`
- `scan_profile_check`
- `ieee80211_parse_wpa`
- `ieee80211_parse_rsn`
- `ieee80211_parse_wapi`
- `cnx_bss_alloc`
- `cnx_update_bss_more`

That made the earlier app-side zero counts from `profile_wrap_diag`, `parse_wrap_diag`, and `bss_wrap_diag` non-authoritative.

The app build now wraps those symbols explicitly.

## Corrected Static Check

Rebuilt app image:

- `target/xtensa-esp32-none-elf/release/meditamer`

Relevant symbols now exist in the app image:

- `__wrap_scan_parse_beacon` at `0x400d75a0`
- `__wrap_ieee80211_parse_wpa` at `0x400d7348`
- `__wrap_cnx_bss_alloc` at `0x400def88`

Most important call-site proof:

- `sta_recv_mgmt` now calls `__wrap_scan_parse_beacon`, not the raw symbol.
- Disassembly line:
  - `4012d781: call8 400d75a0 <__wrap_scan_parse_beacon>`

This closes the earlier objection that app-side zero `profile_wrap_diag` counts might just be a dead wrapper seam.

## Corrected Runtime Capture

Artifact:

- [logs/flash_capture_20260319_103200_longwrap/capture.log](../../logs/flash_capture_20260319_103200_longwrap/capture.log)

Environment:

- boot-scan diag enabled
- explicit IDF compare enabled
- explicit-first enabled
- corrected current-substrate timer-slot trampoline retarget enabled

## App Result After Correcting the Wraps

The app still lands in the same stable zero-result branch:

- `scan_rc=0`
- `scan_done_count=1`
- `scan_done_ap_num=0`
- `scan_list_snapshot ... scannum=0x0000`
- `head_ptr=0x00000000`
- `blob_scan ... word_114=0x00000080`
- `blob_chm ... op_chan=0xff`

At the same postcall checkpoint, the corrected wrap counters remain zero:

- `parse_wrap_diag after=idf_explicit_compare_postcall count=0`
- `bss_wrap_diag after=idf_explicit_compare_postcall count=0`
- `profile_wrap_diag after=idf_explicit_compare_postcall count=0`

## Comparator Control Still Differs

Comparator references:

- [logs/flash_capture_20260317_profile_wrap_comparator/capture.log](../../logs/flash_capture_20260317_profile_wrap_comparator/capture.log)
- [logs/flash_capture_20260317_send_recv_family_comparator_aligned/capture.log](../../logs/flash_capture_20260317_send_recv_family_comparator_aligned/capture.log)

Comparator still shows:

- populated list before retrieval
- `scannum=0x0005` or higher
- non-null `head_ptr`
- `ap_num=5`
- `profile_wrap_diag ... count=8`
- `profile_wrap_diag_entry ... fn=scan_parse_beacon`

## Narrowed Boundary

With the corrected app wraps in place, the zero app-side `profile_wrap_diag` count is now meaningful.

The active boundary is therefore stricter than before:

- scan setup succeeds
- scan completion succeeds
- timer/callback recovery succeeds
- `ScanDone` fires
- but the app does not reach `scan_parse_beacon` during the explicit scan window
- therefore it also never reaches the wrapped parse helpers or the wrapped `cnx_*` list builders

This moves the unresolved boundary earlier than `scan_parse_beacon` and earlier than list linking.

## Next Step

Instrument `sta_recv_mgmt` directly on the app path.

Reason:

- if `sta_recv_mgmt` is not reached, the live split is before management-frame admission/dispatch
- if `sta_recv_mgmt` is reached but `scan_parse_beacon` is not, the split is inside `sta_recv_mgmt` or the immediately preceding `ieee80211_parse_beacon` path
