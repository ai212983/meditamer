# Wi-Fi History Narrowing Follow-up 29

Date: 2026-03-19

## Purpose

Close the remaining ambiguity around station RX callback registration versus invocation on the current app path.

## Runtime Artifact

- [logs/flash_capture_20260319_110200_rxcbptr/capture.log](../../logs/flash_capture_20260319_110200_rxcbptr/capture.log)

## Key Runtime Facts

At `idf_explicit_compare_postcall`, the app still shows the same zero-result scan branch:

- `scan_rc=0`
- `scan_done_count=1`
- `scan_done_ap_num=0`
- `scannum=0x0000`
- `head_ptr=0x00000000`
- `word_114=0x00000080`

The corrected lower-path counters remain zero:

- `profile_wrap_diag after=idf_explicit_compare_postcall count=0`
- `sta_rx_cb_wrap_diag after=idf_explicit_compare_postcall count=0`
- `sta_recv_wrap_diag after=idf_explicit_compare_postcall count=0`

The live RX callback globals are not zero:

- `sta_rxcb_ptr=0x400f8940`
- `ap_rxcb_ptr=0x400f88a4`
- `ndp_rxcb_ptr=0x00000000`

## Static Resolution Of `sta_rxcb_ptr`

From the rebuilt app image:

- `0x400f8940` resolves to `esp_radio::wifi::recv_cb_sta`
- `0x400f88a4` resolves to `esp_radio::wifi::recv_cb_ap`
- blob `sta_rx_cb` remains a separate symbol at `0x400868bc`
- our wrapped blob seam `__wrap_sta_rx_cb` exists at `0x400d7860`

## Meaning

This closes the registration question.

On the current app path:

- the STA RX callback is registered
- it is registered to the Rust shim `esp_radio::wifi::recv_cb_sta`
- but the callback is never invoked in the failing explicit-scan window

That matches the runtime counters:

- `rx_sta=0`
- `sta_rx_cb_wrap_diag count=0`
- `sta_recv_wrap_diag count=0`

So the remaining failure boundary is now:

- after RX callback registration
- before the registered STA RX callback is invoked

## Best Current Boundary

The unresolved split is now inside blob/internal RX dispatch or admission, specifically before delivery into the registered STA RX callback.

That is earlier than:

- `sta_rx_cb`
- `sta_recv_mgmt`
- `scan_parse_beacon`
- parse helpers
- list materialization

## Practical Stop Condition

Further progress on this branch will require deeper blob-facing RX-dispatch instrumentation.

The cheap outer seams are exhausted.

Useful next targets, if we continue later:

- blob-side RX dispatcher immediately above `sta_rxcb`
- management-frame admission filters before callback invocation
- any packet-type gate between ISR/RX list handling and callback delivery
