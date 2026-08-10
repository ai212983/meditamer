# 2026-03-19 Wi-Fi History Narrowing Followup 39

## Goal
Push the strongest remaining non-hardware discriminator at the RX-delivery seam:
- compare the working comparator and the previously forced app branch at `wdevProcessRxSucDataAll`
- use enriched wrapper logging where possible
- separate meaningful runtime fields from wrapper noise

## Context
The active runtime boundary coming into this step was:
- working comparator reaches `lmacProcessRxSucData -> lmacRxDone -> ppEnqueueRxq`
- app can be forced far enough to hit `lmacProcessRxSucData` and `wdevProcessRxSucDataAll`
- but still never reaches `sta_rx_cb`, `sta_recv_mgmt`, or scan-result materialization

Existing app-side forced-branch reference:
- `logs/flash_capture_20260319_force_event_sequence_windowed_app/capture.log`

New comparator-side enriched capture:
- `logs/flash_capture_20260319_comparator_rxdispatch_words/capture.log`

Additional app-side recapture attempts from this step:
- `logs/flash_capture_20260319_force_event_sequence_windowed_words_app_long/capture.log`
- `logs/flash_capture_20260319_force_event_sequence_windowed_words_app_retarget_long/capture.log`
- `logs/flash_capture_20260319_force_event_sequence_windowed_words_app_retarget_60s/capture.log`

## Instrumentation in this step
Extended `wdev_rx_wrap_diag` on both images to log:
- `arg3_words`
- `ret_words`

Touched files:
- `src/firmware/storage/upload/wifi/connect/rx_dispatch_wrap_diag.rs`
- `tools/esp_wifi_legacy_nostd_control/src/rx_dispatch_wrap_diag.rs`
- `tools/esp_wifi_legacy_nostd_control/build.rs`
- `tools/esp_wifi_legacy_nostd_control/src/main.rs`

## What Was Proven

### 1. The working comparator side now has an enriched `wdev_rx` signature
From `logs/flash_capture_20260319_comparator_rxdispatch_words/capture.log` at `idf_explicit_compare postcall`:
- `lmac_rx_suc_wrap_diag ... count=8`
- `wdev_rx_wrap_diag ... count=8`
- all `wdev_rx` entries still show:
  - `arg2=0xffffffff`
  - `ret=0x400dd540`
- `arg3` is mostly `0x000000ff`, with some entries at `0x400dea48`
- `ret_words` are stable across all 8 entries:
  - `8100a136:0898cc1d:b897480c:99090c36`

### 2. The `ret` field is not an RX object discriminator
Static symbol resolution shows:
- app-side `ret` from the older forced branch (`0x400fbd20` in that older build shape) matches the app OSI interrupt-restore callback family
- comparator-side `ret=0x400dd540` resolves to:
  - `esp_wifi::wifi::os_adapter::wifi_int_restore`

So the `ret` field is not a payload/result object. It is effectively an interrupt-restore callback pointer or equivalent OSI callback-family artifact.

This also means the enriched comparator `ret_words` are not the RX object we want. They are code bytes behind that callback pointer.

### 3. The meaningful runtime delta at `wdevProcessRxSucDataAll` remains `arg2` and `arg3`
Working comparator (`logs/flash_capture_20260319_comparator_rxdispatch_words/capture.log`):
- `arg2=0xffffffff`
- `arg3=0x000000ff` for most entries, with some `0x400dea48`
- `ret=wifi_int_restore`

Previously forced app branch (`logs/flash_capture_20260319_force_event_sequence_windowed_app/capture.log`):
- `arg2` is a changing small ordinal set:
  - `0x09, 0x14, 0x15, 0x1f, 0x20, 0x2a, 0x2b, 0x35`
- `arg3=0x3ffca804` in all 8 entries
- `ret` matches the app OSI interrupt-restore callback family

So the strongest surviving runtime difference at this seam is now:
- comparator: `arg2=-1`, `arg3` non-RAM constant/code-like values
- forced app branch: `arg2` small evolving ordinals, `arg3` stable RAM pointer

### 4. The enriched app-side forced branch did not reproduce on the current tree
I reran the app under:
- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_FIRST=1`
- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE=1`
- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_SHOW_HIDDEN=1`
- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_FORCE_COMPARATOR_EVENT_SEQUENCE=1`

Then again with the historical retarget shape recovered from Followup 30:
- `MEDITAMER_WIFI_NAN_TIMER_SLOT_RETARGET_DIAG=1`
- `MEDITAMER_WIFI_NAN_TIMER_SLOT_RETARGET_TRAMPOLINE_DIAG=1`

Results:
- the plain force-window run reached explicit compare but did not reproduce the older `wdev_rx` branch
- the recovered retarget+trampoline shape reached `idf_explicit_compare_prestart` and `idf_explicit_compare_postcall`, but still did not emit the older forced-branch `wdev_rx` entries in the current instrumented tree
- therefore the enriched app-side `arg3_words` / `ret_words` data was not recovered in this step

## Interpretation
Two useful conclusions survived this attempt:

1. The previously captured app/comparator `wdev_rx` split is still meaningful, but the `ret` field is not.
2. The surviving discriminators at this seam are the live call arguments, especially:
   - `arg2`
   - `arg3`

The enriched comparator run improved the interpretation of the seam, but the app-side reenactment drifted and did not reproduce the old forced branch on the current tree.

## Practical Boundary
Without debugger hardware, the strongest honest statement is now:
- the working comparator and the previously forced app branch differ materially at `wdevProcessRxSucDataAll`
- the meaningful difference is in live arguments entering that seam, not in the return value
- current instrumentation changes are enough that the old forced app branch is no longer stably reproducible for enriched capture

## Blocker
The next useful step is no longer another wrapper permutation.

Without JTAG/debug hardware, the remaining non-hardware choices are:
- lower-perturbation binary patch/trampoline instrumentation around `wDev_ProcessRxSucData` / `wdevProcessRxSucDataAll`
- or static analysis of how app vs comparator prepare `arg2` / `arg3` for that seam

Further high-verbosity wrapper growth is now likely to perturb the branch more than it explains it.
