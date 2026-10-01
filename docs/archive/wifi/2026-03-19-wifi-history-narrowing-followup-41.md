# 2026-03-19 Wi-Fi History Narrowing Follow-up 41

## Goal
Use a minimal perturbation-reduction build to test whether the forced RX-delivery branch can still reach `wdevProcessRxSucDataAll` when the heavier downstream wrapper stack is removed.

## Build Change
A new build-time gate now exists in `build.rs`:

- `MEDITAMER_WIFI_RX_RECOVERY_MINIMAL_DIAG=1`

When enabled, only these linker wraps remain active:

- `wDev_ProcessFiq`
- `hal_mac_interrupt_get_event`
- `hal_mac_interrupt_clr_event`
- `wdevProcessRxSucDataAll`

The downstream `wDev_ProcessRxSucData` wrapper module is excluded under the same cfg so the reduced build can link cleanly.

## Runs

### 60 s capture
- Log: [flash_capture_20260319_minimal_rx_recovery_forced_app/capture.log](../../logs/flash_capture_20260319_minimal_rx_recovery_forced_app/capture.log)
- Result: the run did not reach a useful postcall boundary inside the 60 s window.
- That result alone was ambiguous.

### 120 s capture
- Log: [flash_capture_20260319_minimal_rx_recovery_forced_app_120s/capture.log](../../logs/flash_capture_20260319_minimal_rx_recovery_forced_app_120s/capture.log)
- This longer run reached the stable zero-result branch again.

## What the 120 s capture proved

1. The minimal build still reaches scan completion.
- `event scan_done_list status=0 count=0 scan_id=128`
- `event scan_done status=0 count=0 scan_id=128`

2. The result list still never materializes.
- `scan_list_snapshot label=event_post_before_get_ap_num scannum=0x0000 head_ptr=0x00000000`
- `scan_list_snapshot label=event_post_after_get_ap_num scannum=0x0000 head_ptr=0x00000000`

3. RX delivery is still not reaching the downstream seam.
- `wdev_rx_wrap_diag after=after_nan_timer_slot_retarget count=0`
- No `wdevProcessRxSucDataAll` entries were observed before the final `ScanDone` path.

4. The reduced wrapper stack did not recover the old forced RX-delivery branch.
- This closes the hypothesis that the heavier downstream instrumentation alone was suppressing entry into `wdevProcessRxSucDataAll`.

## Updated Boundary
The surviving failure boundary remains earlier than `wdevProcessRxSucDataAll`, even under the reduced instrumentation build.

Current narrowed path:

- `wDev_ProcessFiq` is live
- `hal_mac_interrupt_get_event` / `clr_event` are live
- scan still completes with `scan_id=128`
- the AP-result list remains empty
- RX delivery still never reaches `wdevProcessRxSucDataAll`

## Conclusion
The minimal perturbation-reduction build did not revive the downstream RX-delivery seam.

That means the active failure is still upstream of `wdevProcessRxSucDataAll`, not an artifact of the heavier downstream wrapper stack.

## Next Step
Do not spend more time trying to revive `wdevProcessRxSucDataAll` with wrapper reduction alone.

The next honest non-hardware step would be one of:

1. binary-patch or trampoline an earlier RX admission seam above `wdevProcessRxSucDataAll`
2. or stop source-level narrowing and wait for JTAG/debug hardware to inspect the MAC event producer path directly
