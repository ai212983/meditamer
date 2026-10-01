## 2026-03-19: `wDev_ProcessFiq` only sees `0x800`, never reaches RX-end handling

### New artifacts

- `logs/flash_capture_20260319_104821/capture.log`
- `logs/flash_capture_20260319_105347/capture.log`

### What changed

- Added direct wrappers for:
  - `wDev_ProcessFiq`
  - `hal_mac_interrupt_get_event`
  - `hal_mac_interrupt_clr_event`
  - `hal_mac_rx_get_end_info`
  - `pp_post`
  - `lmacProcessRxSucData`
- Added one more attempted wrapper for:
  - `wdev_process_panic_watchdog`

### What is now proven

1. `WIFI_MAC()` is dispatching into wrapped `wDev_ProcessFiq`.

- In `logs/flash_capture_20260319_104821/capture.log`:
  - `blob_wifi_mac_isr after=after_start_pre_driver_state target_ptr=0x400deeec`
  - `wdev_fiq_wrap_diag after=after_start_pre_driver_state count=3`
- In `logs/flash_capture_20260319_105347/capture.log`:
  - `blob_wifi_mac_isr after=after_start_pre_driver_state target_ptr=0x400e31e0`
  - `wdev_fiq_wrap_diag after=after_start_pre_driver_state count=3`

So the top-half ISR seam is genuinely live and interposable.

2. The only observed interrupt-event mask is `0x00000800`.

- `hal_mac_get_event_wrap_diag` shows only alternating:
  - `ret=0x00000800`
  - `ret=0x00000000`
- `hal_mac_clr_event_wrap_diag` clears only:
  - `arg0=0x00000800`

This holds both before and after the explicit scan window.

3. The RX-end branch is not being taken.

- `hal_mac_rx_end_wrap_diag ... count=0`
- `lmac_rx_suc_wrap_diag ... count=0`
- `lmac_rx_done_wrap_diag ... count=0`
- `ppenq_wrap_diag ... count=0`

So this path is not reaching:

- `hal_mac_rx_get_end_info`
- `lmacProcessRxSucData`
- `lmacRxDone`
- `ppEnqueueRxq`

4. The only visible `pp_post` traffic at this seam is generic control posting.

- `pp_post_wrap_diag` stays on:
  - `arg0=0x00000006`
- No RX-family `pp_post(14, ...)` or scan/RX queue ingress shows up in this failing window.

5. The scan still completes on the app path despite that missing RX-end path.

- `scan_done_eventpost after=rust_scan_settled count=1 status=0 scan_id=128 ap_num=0`
- `blob_scan after=rust_scan_settled ... word_114=0x00000080`

So the stable failing branch is now:

- scan completes
- result list remains empty
- top-half ISR never takes the RX-end branch

### Static interpretation

Disassembly of `wDev_ProcessFiq` in the app image shows:

- bit `11` gates the call to `wdev_process_panic_watchdog`
- the RX-end path is behind a later mask check before `hal_mac_rx_get_end_info`

Given the runtime mask above, the app is only seeing the bit-11 watchdog event and not the later RX-end event family.

### Important failed probe

- `wdev_panic_watchdog_wrap_diag ... count=0`

This does **not** mean the watchdog path is dead.

It means the call from real `wDev_ProcessFiq` to `wdev_process_panic_watchdog` is a direct local call inside the blob image, so linker `--wrap` does not interpose there.

### Current boundary

The surviving boundary is now:

- after `WIFI_MAC()` ISR dispatch
- inside `wDev_ProcessFiq`
- before RX-end handling (`hal_mac_rx_get_end_info`)

More precisely:

- the app top-half ISR only sees `0x800`
- it never sees the RX-end event family needed to enter the RX delivery path

### What this closes

- “the ISR target is wrong”
- “`wDev_ProcessFiq` is not running”
- “`hal_mac_rx_get_end_info` is running but later delivery fails”
- “`lmacProcessRxSucData` / `lmacRxDone` are the first live failing branch”

### Best next step

The next useful step is no longer another outer linker wrap in the app firmware.

It is one of:

1. Run the same `hal_mac_interrupt_get_event` probe on the working legacy comparator and compare event masks.
2. Use binary-patch or breakpoint-style instrumentation inside app `wDev_ProcessFiq`.
3. Instrument the hardware/MAC interrupt-enable state feeding `hal_mac_interrupt_get_event`.

Of these, `1` is the least invasive remaining discriminator.
