# Wi-Fi History Narrowing Follow-up 28

Date: 2026-03-19

## Purpose

Record the first app-side measurement of `sta_recv_mgmt` after correcting the lower wrap instrumentation.

## Runtime Artifact

- [logs/flash_capture_20260319_104100_starecv/capture.log](../../logs/flash_capture_20260319_104100_starecv/capture.log)

Environment matched Follow-up 27:

- boot-scan diag enabled
- explicit IDF compare enabled
- explicit-first enabled
- corrected current-substrate timer-slot trampoline retarget enabled

## Result

The app still lands in the same stable zero-result branch:

- `scan_rc=0`
- `scan_done_count=1`
- `scan_done_ap_num=0`
- `scan_list_snapshot ... scannum=0x0000`
- `head_ptr=0x00000000`
- `blob_scan ... word_114=0x00000080`
- `blob_chm ... op_chan=0xff`

The corrected lower wrappers remain zero:

- `parse_wrap_diag after=idf_explicit_compare_postcall count=0`
- `bss_wrap_diag after=idf_explicit_compare_postcall count=0`
- `profile_wrap_diag after=idf_explicit_compare_postcall count=0`

New direct measurement:

- `sta_recv_wrap_diag after=idf_explicit_compare_postcall count=0`

## Interpretation

This is stricter than Follow-up 27.

The live failure boundary is no longer merely “before `scan_parse_beacon`.”
It is now:

- before `sta_recv_mgmt`
- therefore before management-frame dispatch into the scan/beacon path

So, in the stable zero-result branch:

- scan setup succeeds
- scan timing/progression succeeds
- `ScanDone` fires
- but the app does not enter `sta_recv_mgmt` during the explicit scan window
- therefore it also never enters `scan_parse_beacon`, the WPA/RSN/WAPI parse helpers, or the `cnx_*` list builders

## Current Best Boundary

The unresolved split is now between:

- successful scan/channel progression
- and management-frame admission/dispatch into `sta_recv_mgmt`

This is earlier than result-list materialization and earlier than beacon/profile parsing.

## Next Step

Static and runtime focus should move to the immediate caller chain above `sta_recv_mgmt`.

Most useful target:

- the RX management-frame dispatch path that decides whether to call `sta_recv_mgmt`

If that path has no stable named symbol boundary, further progress will require deeper blob-facing RX dispatch instrumentation rather than more scan-command or timer probes.
