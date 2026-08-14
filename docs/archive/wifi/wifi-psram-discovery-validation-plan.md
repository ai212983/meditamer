# Wi-Fi PSRAM Discovery Validation Plan

> Historical record. The `psram-alloc`, `esp-hal-runtime`, and `graphics`
> features named below have been retired; PSRAM, the ESP-HAL runtime, and LVGL
> are now unconditional. See [Compile-Time Features](./compile-time-features.md)
> for the current build contract.

## Goal

Determine whether PSRAM is the primary cause of the current Wi-Fi discovery
failure on this device.

This plan exists because discovery previously worked on the same hardware
before PSRAM was introduced, while the current validation baseline shows:

- `backend_legacy_port` active
- init completes
- `start=ok`
- `WIFI_MAC` interrupts rise
- pre-scan promisc stays zero
- `wifi_rx_cb_count sta=0 ap=0`
- direct explicit scan returns `ap_num=0`
- wrapped scan returns `InternalError(Timeout)`

## Hypothesis

PSRAM integration changed memory placement in a way that broke Wi-Fi-critical
discovery paths, especially around buffers or state touched by the radio or
callback-adjacent code.

## Validation Rules

Use only:

- canonical full-flash `hostctl flash-capture`
- `backend_legacy_port` diagnostics
- the same boot-scan flow already used for Wi-Fi validation

Keep everything else constant while changing only PSRAM participation.

## Success Criteria

Treat PSRAM as confirmed if disabling it causes any of these to move off zero:

- pre-scan promisc totals
- `wifi_rx_cb_count`
- `scan_done_eventpost`
- direct explicit scan `ap_num`
- wrapped scan AP count

If all stay unchanged, PSRAM is not the primary blocker.

## Phases

- [x] Phase 1: prove or disprove PSRAM correlation
- [ ] Phase 2: localize which memory paths must remain internal
- [x] Phase 3: validate against an official ESP-IDF control
- [x] Phase 4: decide the long-term fix

## Phase 1: Prove Or Disprove PSRAM Correlation

### Goal

Disable PSRAM while holding the Wi-Fi validation path constant.

### Steps

- [x] Step 1.1 identify the narrowest build/config seam that disables PSRAM
- [x] Step 1.2 run canonical validation with PSRAM disabled
- [x] Step 1.3 compare the resulting discovery metrics against the PSRAM-on
      baseline

### Planned Implementation

Current repo seam:

- feature `psram-alloc` enables PSRAM allocator support
- default features currently include `psram-alloc`

First proof build:

- disable default features
- enable only:
  - `esp-hal-runtime`
  - `asset-upload-http`
  - `asset-upload-http-pipeline`
  - `wifi-debug-slim-app`

This is intended to preserve the current Wi-Fi validation path while removing
PSRAM participation.

### Notes

- commit:
- validation:
  `logs/hostctl_flashcapture_no_psram_20260316_101638/capture.log`
- outcome:
  - identified `psram-alloc` as the narrow repo seam that enables PSRAM
  - added the missing `#[cfg(feature = "psram-alloc")]` guard to
    `src/firmware/psram/buffer.rs`
    so the no-PSRAM build path compiles correctly
  - validated with:
    - `CARGO_NO_DEFAULT_FEATURES=1`
    - `CARGO_FEATURES='esp-hal-runtime,graphics,asset-upload-http,asset-upload-http-pipeline,wifi-debug-slim-app'`
  - canonical no-PSRAM result:
    - `psram: feature_enabled=false state=Disabled`
    - backend still `backend-legacy-port`
    - `runtime_init result=ok`
    - `legacy_port_wifi_init stage=done`
    - `start=ok`
    - pre-scan promisc stayed zero
    - `wifi_rx_cb_count sta=0 ap=0`
    - `scan_done_eventpost count=0`
    - direct null scan still `scan_rc=12300`
    - direct explicit scan still `ap_num=0`
    - wrapped scan still `InternalError(Timeout)`
  - conclusion: PSRAM is not the primary blocker for discovery

## Phase 2: Localize Internal-RAM Requirements

### Goal

If Phase 1 confirms PSRAM correlation, determine which paths must stay in
internal RAM.

### Steps

- [ ] Step 2.1 keep Wi-Fi driver-adjacent buffers internal
- [ ] Step 2.2 keep callback-adjacent queue/buffer state internal
- [ ] Step 2.3 keep scan/result storage internal
- [ ] Step 2.4 reintroduce PSRAM only for cold bulk buffers

### Notes

- commit:
- validation:
- outcome:

## Phase 3: Official ESP-IDF Control

### Goal

Separate a board/PSRAM/system issue from an esp-rs integration issue.

### Steps

- [x] Step 3.1 run an official ESP-IDF station scan example on the same board
- [x] Step 3.2 compare PSRAM-off vs PSRAM-on behavior there
- [x] Step 3.3 classify whether the problem is substrate-level or integration-level

### Notes

- commit:
- validation:
  - PSRAM-off:
    `logs/esp_idf_wifi_control_psram_off_20260316_102928/serial_capture.log`
  - PSRAM-on:
    `logs/esp_idf_wifi_control_psram_on_20260316_103325/serial_capture.log`
- outcome:
  - the official C/ESP-IDF control app still scans successfully with PSRAM off
    and with PSRAM on
  - PSRAM-off control result:
    - `wifi_init nvs_enable=1`
    - `mode=scan_only`
    - `pre_scan_driver_state ... ps=1 ... cc=01.`
    - `scan_complete total_ap_count=9 returned_ap_count=9`
  - PSRAM-on control result:
    - `esp_psram: Found 8MB PSRAM device`
    - `esp_psram: PSRAM initialized`
    - `esp_psram: Adding pool of 4096K of PSRAM memory to heap allocator`
    - `esp_psram: Reserving pool of 32K of internal memory for DMA/internal allocations`
    - `wifi_init nvs_enable=1`
    - `mode=scan_only`
    - `pre_scan_driver_state ... ps=1 ... cc=01.`
    - `scan_complete total_ap_count=10 returned_ap_count=10`
  - repo-side test seam added:
    - `tools/esp_idf_wifi_control/sdkconfig.psram_on.defaults`
  - conclusion:
    - PSRAM does not break official ESP-IDF station scanning on this board
    - the remaining failure is integration-level and specific to the current
      no-std / `esp-radio` / backend path, not a generic board+PSRAM substrate
      failure

## Phase 4: Decide The Long-Term Fix

### If PSRAM Is Confirmed

- [ ] Step 4.1 keep discovery-critical Wi-Fi memory in internal RAM
- [ ] Step 4.2 allow PSRAM only for cold bulk app buffers
- [ ] Step 4.3 consider a dedicated discovery mode if memory pressure remains

### If PSRAM Is Not Confirmed

- [x] Step 4.4 stop the PSRAM branch
- [x] Step 4.5 return to substrate/backend strategy selection

### Notes

- commit:
- validation:
  `logs/hostctl_flashcapture_no_psram_20260316_101638/capture.log`
- outcome:
  - stop condition reached for the PSRAM hypothesis
  - disabling PSRAM did not restore any discovery metric
  - PSRAM may still affect memory pressure, but it is not the primary cause of
    the current discovery failure
  - recommended next line of work is back in substrate/backend selection, not
    PSRAM placement

## Current Next Step

Do not continue Phase 2 on this branch.

Phase 3 is now closed.

The next meaningful step is no longer PSRAM-focused. It is to return to the
Wi-Fi backend/substrate line of work with this stronger conclusion:

- official ESP-IDF discovery works with PSRAM off
- official ESP-IDF discovery works with PSRAM on
- the remaining fault is specific to the current no-std Rust Wi-Fi integration
