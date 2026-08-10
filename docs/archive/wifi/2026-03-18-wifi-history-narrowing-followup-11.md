# 2026-03-18 Wi-Fi History Narrowing Follow-up 11

## Scope

This follow-up closes the ambiguity around the paired `chm_init` timer-slot retarget experiment.

It answers two questions:

1. Does the paired retarget actually change the live timer callback identity, or only the compat bookkeeping?
2. Does the branch flip to `scan_rc=12300` require later execution of the retargeted callback, or does it happen earlier?

## Artifacts

- Baseline app live-timer capture:
  - `logs/flash_capture_20260318_134728/capture.log`
- First paired-retarget run with stale handle interpretation:
  - `logs/flash_capture_20260318_135127/capture.log`
- Corrected paired-retarget run using `ets_timer.priv_` as the live timer handle:
  - `logs/flash_capture_20260318_135309/capture.log`

## Code Changes In This Slice

- Added real esp-rtos timer live-state exports in:
  - `vendor/esp-rtos-0.2.0/src/esp_radio/timer_queue.rs`
  - `vendor/esp-rtos-0.2.0/src/esp_radio/mod.rs`
  - `vendor/esp-rtos-0.2.0/src/lib.rs`
- Added esp-radio shim for that live-state API in:
  - `vendor/esp-radio-0.17.0/src/lib.rs`
- Switched slot logging to the real timer handle behind `ets_timer.priv_` in:
  - `src/firmware/storage/upload/wifi/connect/nan_timer_slot_retarget_diag.rs`

## Symbol Resolution

Resolved with the ESP Xtensa binutils from the local Rust toolchain:

- `0x40121cbc` -> `nan_dp_schedule_ndc_start`
- `0x40132b00` -> `ieee80211_timer_process`

This is the relevant callback-family swap in the paired-retarget experiment.

## Runtime Result

### Baseline app

At `idf_explicit_compare_postcall` in `logs/flash_capture_20260318_134728/capture.log`:

- `scan_rc=0`
- `blob_chm op_chan=0xff`
- `blob_scan word_00=0x00000000`
- timer slot live state:
  - slot 0 callback: `0x40121ca8`
  - slot 1 callback: `0x40121ca8`
  - both slots have valid started/next-due times in the explicit-scan window
- timer execution is visible in the same window:
  - `timer_exec_recent ... callback_ptr=0x40121ca8`
- timer-tagged queue sends are also visible:
  - `wifi_os_diag_send_recent ... timer_callback_ptr=0x40121ca8`

Interpretation:

- Baseline app keeps the original NAN-family callback live in both slots.
- That callback both executes and injects timer-tagged queue work during the successful-admission / empty-results failure family.

### Paired retarget, corrected live-handle run

At `after_nan_timer_slot_retarget` in `logs/flash_capture_20260318_135309/capture.log`:

- `matched_count=2 retargeted_count=2`
- slot 0 live callback: `0x40132b00`
- slot 1 live callback: `0x40132b00`
- both slots now point to new timer handles:
  - slot 0 handle `0x3ffc0378`
  - slot 1 handle `0x3ffc03b0`
- recent compat `setfn` entries confirm the recreated timers:
  - ordinal 4: `ets_timer_ptr=0x3ffc9bcc timer_handle_ptr=0x3ffc0378 callback_ptr=0x40132b00 arg_ptr=0x0`
  - ordinal 5: `ets_timer_ptr=0x3ffc9be0 timer_handle_ptr=0x3ffc03b0 callback_ptr=0x40132b00 arg_ptr=0x1`

At `idf_explicit_compare_postcall` in the same capture:

- `scan_rc=12300`
- `blob_chm op_chan=0x01 ptr_08=0xa ptr_0c=0x14`
- `blob_scan word_00=0x0000010f word_30=0x14 word_34=0x0a`
- slot live state still shows the retargeted callback in both slots:
  - slot 0 callback: `0x40132b00`
  - slot 1 callback: `0x40132b00`
  - both slots have valid started/next-due times
- but there is still no timer execution evidence in this postcall window:
  - no `timer_exec_recent after=idf_explicit_compare_postcall`
  - no timer-tagged queue sends at postcall

Interpretation:

- The paired retarget is real at the live timer-object level.
- It is not just compat bookkeeping drift.
- The app flips into the earlier `scan_rc=12300` failure form before any retained postcall execution evidence for the retargeted callback appears.

## Narrowed Boundary

This closes another ambiguity.

The remaining live boundary is now:

- not the esp-rtos timer object model itself
- not whether the retarget changed real callback identity
- but the compat/channel-manager path that consumes the slot callback identity before or during scan admission

Current strongest statement:

- the exact paired `chm_init` slot callback identity is causally upstream of the branch split
- replacing both slots with `ieee80211_timer_process` is sufficient to move the app from:
  - `scan_rc=0` + empty results
  - to `scan_rc=12300` + no `ScanDone`
- that branch flip occurs without needing visible postcall execution of the retargeted callback

## Practical Next Step

The next useful target is no longer the timer object.

It is the compat/channel-manager consumer path that uses these slot identities:

1. compare baseline vs paired-retarget at the first compat-layer arm/use site after `chm_init`
2. focus on the path that converts:
   - baseline callback family `nan_dp_schedule_ndc_start`
   - into the later `op_chan=0xff` / `scan_rc=0` path
3. compare that against the paired-retarget path that yields:
   - `ieee80211_timer_process`
   - `op_chan=0x01`
   - `word_00=0x10f`
   - `scan_rc=12300`
