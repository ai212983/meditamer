# 2026-03-19 Wi-Fi History Narrowing Follow-up 40

## Goal

Advance the non-hardware fallback from wrapper-only probing to a lower-perturbation
binary trampoline at the direct `wDev_ProcessRxSucData` callsite.

## Why This Follow-up Exists

Follow-up 39 ended with the recommendation to try a lower-perturbation
binary patch/trampoline around `wDev_ProcessRxSucData` /
`wdevProcessRxSucDataAll`, because wrapper-level instrumentation had reached its
perturbation limit.

## What Was Proven

### 1. Plain linker wrapping does not interpose the direct local blob call

I added `--wrap=wDev_ProcessRxSucData` plus dedicated wrap modules on both app
and comparator sides.

Build verification showed:

- app release image still contained a direct call inside `wdevProcessRxSucDataAll`:
  - `4008975e: call8 40089330 <wDev_ProcessRxSucData>`
- comparator release image still contained the same direct call shape:
  - `40086e67: call8 40086acc <wDev_ProcessRxSucData>`

So plain linker wrapping is not enough for this seam.

### 2. The wrap symbol can be kept alive and targeted by a binary patch

I forced the wrapper symbol to remain in the linked image with:

- `pub unsafe extern "C" fn __wrap_wDev_ProcessRxSucData(...)`
- `#[used]` static anchor referencing that function

Resulting live app symbols in the current forced build:

- `40089330 T wDev_ProcessRxSucData`
- `400896d4 T wdevProcessRxSucDataAll`
- `400d6014 T __wrap_wDev_ProcessRxSucData`

### 3. A direct `call8` binary patch is statically viable

Xtensa `call8` encoding at this seam uses:

- base = `((callsite & ~0x3) + 4)`

A corrected copied app ELF was patched successfully at the real `call8` site:

- artifact:
  - `logs/binary_patch_tests/meditamer_wdev_rx_patch_forced_currentaddrs_fixed.elf`
- patched disassembly:
  - `4008975e: call8 400d6014 <__wrap_wDev_ProcessRxSucData>`

So the binary-trampoline path is technically viable without JTAG.

## Important Runtime Result

### 4. The current forced-build baseline does not reach the patched seam anymore

I rebuilt the current forced-RX diagnostic shape with:

- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1`
- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_FIRST=1`
- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE=1`
- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_SHOW_HIDDEN=1`
- `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_FORCE_COMPARATOR_EVENT_SEQUENCE=1`
- `MEDITAMER_WIFI_NAN_TIMER_SLOT_RETARGET_DIAG=1`
- `MEDITAMER_WIFI_NAN_TIMER_SLOT_RETARGET_TRAMPOLINE_DIAG=1`

Then I flashed the resulting baseline build and captured:

- `logs/flash_capture_20260319_wdev_process_rx_patch_forced_app/capture.log`

Key result from that capture:

- `wdev_process_rx_wrap_diag after=after_nan_timer_slot_retarget count=0`
- `wdev_process_rx_wrap_diag after=after_start_pre_driver_state count=0`
- no later `wdev_process_rx_wrap_diag_entry` lines at all
- no `wdev_rx_wrap_diag` entries at the explicit-compare window either

At the same time, the run still completed the familiar zero-result branch:

- `scan_done_list status=0 count=0 scan_id=128 scannum=0x0000 head_ptr=0x0`
- `idf_explicit_compare_postcall=postcall scan_rc=0 ... scan_done_count=1 ... scan_done_ap_num=0`

That means:

- the current forced-build baseline is not reaching `wdevProcessRxSucDataAll`
- therefore a corrected binary patch at `wDev_ProcessRxSucData` would be
  downstream of the live failure in this build shape

## Why The First Flash Was Not the Real Patch Test

My first attempt to flash a patched forced-build ELF used stale section file
offsets from an earlier build layout.

The function VMAs stayed the same, but the current forced build moved `.rwtext`
/ `.rwtext.wifi` file offsets.

That first flash therefore acted as a refreshed forced-build baseline, not the
real patched-trampoline run.

This was corrected afterward in the copied ELF listed above.

## Current Boundary

The lower-perturbation binary patch route is now in this state:

- technically viable at the ELF/callsite level
- not currently valuable at runtime, because the present forced build never
  reaches that callsite family

So the live blocker is still earlier than `wdevProcessRxSucDataAll` in the
current tree.

## Conclusion

Follow-up 39 recommendation `1` is only partially actionable today:

- yes, a binary trampoline at `wDev_ProcessRxSucData` is feasible
- no, it is not currently the right runtime seam to flash against, because the
  current forced-build branch collapses before that seam is entered

## Best Next Step

Do not spend another flash on the corrected `wDev_ProcessRxSucData` patch until
we first recover a build shape that actually reaches `wdevProcessRxSucDataAll`.

The next useful branch is earlier:

1. recover the forced RX-delivery branch with less perturbation than the current
   wrapper stack, or
2. move earlier than `wdevProcessRxSucDataAll` again and instrument the RX
   admission path that decides whether this seam is reached at all.
