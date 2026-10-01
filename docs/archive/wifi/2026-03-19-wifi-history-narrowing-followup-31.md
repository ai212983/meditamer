# 2026-03-19 Wi-Fi History Narrowing Followup 31

## Goal
Correct the overreach in Followup 30 and record the new top-half ISR boundary.

## Correction to Followup 30
Followup 30 correctly showed zero hits for the added RX wrappers, but its interpretation was too strong.

Static inspection of the release image showed:
- `sta_recv_mgmt` does call `__wrap_scan_parse_beacon`
- `lmacRxDone` does call `__wrap_ppEnqueueRxq`
- but most other RX wrappers were not on blob-internal direct-call paths

That means zero counts on wrappers such as:
- `ppRxPkt`
- `ppProcessRxPktHdr`
- `wdevProcessRxSucDataAll`
- `sta_input`

cannot by themselves prove those paths are dead.

## New Instrumentation
Added a direct `lmacRxDone` wrapper and logged the live `WIFI_MAC()` dispatch target via `ISR_INTERRUPT_1`.

Touched files:
- `build.rs`
- `src/firmware/storage/upload/wifi/connect/lmac_wrap_diag.rs`
- `src/firmware/storage/upload/wifi/connect/blob_state_diag.rs`
- `src/firmware/storage/upload/wifi/connect/boot_scan_diag/mod.rs`
- `src/firmware/storage/upload/wifi/connect/mod.rs`
- `vendor/esp-radio-0.17.0/src/lib.rs`

## Capture
- `logs/flash_capture_20260319_103911/capture.log`

## What Was Proven
### 1. `WIFI_MAC()` is active and dispatching a stable top-half target
From the capture:
- `blob_wifi_mac_isr after=after_start_pre_driver_state target_ptr=0x4008dd98 arg_ptr=0x00000000`
- the same target remains through `rust_scan` and `rust_scan_settled`
- `wifi_mac_isr_count after=rust_scan_settled count=92`

So the Wi-Fi MAC interrupt is firing and the ISR dispatch target is stable.

### 2. `lmacRxDone` is still not reached in the failing zero-result branch
From the same capture:
- `lmac_rx_done_wrap_diag after=rust_scan_settled count=0`
- `ppenq_wrap_diag after=rust_scan_settled count=0`

Since static disassembly shows `lmacRxDone` calls `__wrap_ppEnqueueRxq`, this is meaningful.

### 3. The control branch is still valid
Also from the same capture:
- `scan_done_eventpost after=rust_scan_settled count=1 status=0 ... scan_id=128 ap_num=0`
- `blob_scan after=rust_scan_settled ... word_114=0x00000080`
- `blob_chm after=rust_scan_settled op_chan=0xff`

So the app still reaches the stable zero-result completion branch.

## New Boundary
The strongest live boundary is now:
- `WIFI_MAC()` fires
- it dispatches `ISR_INTERRUPT_1 -> 0x4008dd98`
- but that branch does not reach `lmacRxDone`
- and therefore does not reach RX queue ingress via `ppEnqueueRxq`

This is stronger than the previous “earlier than `ppEnqueueRxq`” statement because it identifies the top-half ISR target that sits between the MAC interrupt and `lmacRxDone`.

## Practical Next Step
Resolve and instrument `0x4008dd98` in the release image.

That function is now the first unresolved top-half RX boundary in the failing scan branch.
