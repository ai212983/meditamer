# 2026-03-19 Wi-Fi History Narrowing Followup 30

## Goal
Push the RX-delivery boundary earlier than the previously dead seams:
- `sta_rx_cb`
- `sta_recv_mgmt`
- `scan_parse_beacon`
- result-list materialization

## Context
The active control shape remained:
- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_FIRST=1`
- `MEDITAMER_WIFI_NAN_TIMER_SLOT_RETARGET_DIAG=1`
- `MEDITAMER_WIFI_NAN_TIMER_SLOT_RETARGET_TRAMPOLINE_DIAG=1`

This preserved the stable branch:
- scan succeeds
- `ScanDone` fires
- `word_114=0x80`
- result list stays empty before retrieval
- `rx_sta=0`

## Instrumentation Added
Added wrapper diagnostics for the generic RX dispatcher path above `sta_rx_cb`:
- `wdevProcessRxSucDataAll`
- `ppProcessRxPktHdr`
- `ppRxPkt`
- `ppRxProtoProc`
- `ppRxFragmentProc`
- `ppEnqueueRxq`
- `ppDequeueRxq_Locked`
- retained existing wrappers for:
  - `sta_input`
  - `sta_rx_cb`
  - `sta_recv_mgmt`
  - `scan_parse_beacon`

Touched files:
- `build.rs`
- `src/firmware/storage/upload/wifi/connect/rx_dispatch_wrap_diag.rs`
- `src/firmware/storage/upload/wifi/connect/mod.rs`
- `src/firmware/storage/upload/wifi/connect/boot_scan_diag/mod.rs`

## Captures
Primary captures:
- `logs/flash_capture_20260319_113900_rxdispatch/capture.log`
- `logs/flash_capture_20260319_115600_pprx_long/capture.log`
- `logs/flash_capture_20260319_121000_pprxq_long/capture.log`

The short `pprx` capture without the long boot window was not used as the main control because it did not include the full explicit-compare window.

## What Was Proven
### 1. The generic RX dispatcher path stays completely dead in the failing scan window
From `logs/flash_capture_20260319_115600_pprx_long/capture.log` at `rust_scan_settled`:
- `wdev_rx_wrap_diag ... count=0`
- `pphdr_wrap_diag ... count=0`
- `pprx_wrap_diag ... count=0`
- `pprx_proto_wrap_diag ... count=0`
- `pprx_frag_wrap_diag ... count=0`
- `sta_input_wrap_diag ... count=0`
- `sta_rx_cb_wrap_diag ... count=0`
- `sta_recv_wrap_diag ... count=0`
- `parse_wrap_diag ... count=0`

### 2. The RX packet queue boundary also stays dead
From `logs/flash_capture_20260319_121000_pprxq_long/capture.log` at `rust_scan_settled`:
- `ppenq_wrap_diag ... count=0`
- `ppdeq_wrap_diag ... count=0`
- all later RX/parse wrappers remain `0`
- `wifi_rx_cb_count after=rust_scan_settled sta=0 ap=0`

### 3. The app still reaches the same zero-result completion branch
From `logs/flash_capture_20260319_121000_pprxq_long/capture.log`:
- `scan_done_eventpost ... count=1 status=0 ... scan_id=128 ap_num=0`
- `scan_list_snapshot ... scannum=0x0000 head_ptr=0x00000000`
- `blob_scan ... word_114=0x00000080`
- `blob_chm ... op_chan=0xff`

So the control branch is still valid; only the RX path above the packet queue is dead.

## New Boundary
The failure boundary is now earlier than all of these:
- `ppEnqueueRxq`
- `ppDequeueRxq_Locked`
- `ppRxPkt`
- `ppRxProtoProc`
- `ppRxFragmentProc`
- `ppProcessRxPktHdr`
- `wdevProcessRxSucDataAll`
- `sta_input`
- `sta_rx_cb`
- `sta_recv_mgmt`
- `scan_parse_beacon`

That is materially stronger than the previous conclusion.

## Interpretation
The failing branch is no longer best described as:
- scan results are parsed but dropped
- scan callbacks are invoked but later list linking fails

It is now better described as:
- scan completes internally
- `ScanDone` is posted
- but the expected RX ingress path for beacon/mgmt packet delivery never becomes visible even at the RX queue boundary

## Blocker
The next unresolved layer is above `ppEnqueueRxq`.

That means the next useful probe is no longer a cheap named scan or RX wrapper. It likely requires one of:
- instrumentation in the blob-side RX producer before queue insertion
- instrumentation at the Wi-Fi MAC/PP producer that should feed `ppEnqueueRxq`
- binary-level breakpoint/trampoline work above the named RX queue functions

## Practical Conclusion
The current narrowing is blocked on deeper blob-facing RX producer instrumentation.

At this point, more wrappers below `ppEnqueueRxq` will not produce new information.
