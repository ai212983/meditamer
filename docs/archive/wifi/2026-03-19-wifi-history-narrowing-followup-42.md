# 2026-03-19 Wi-Fi History Narrowing Follow-up 42

## Goal
Check whether a final top-half ring dump at `ScanDone` could reveal the MAC-event sequence at the exact point where the empty AP list is emitted in the minimal recovery build.

## Change Tried
A temporary hook was added in the `ScanDone` event handler to log:

- `wdev_fiq_wrap_diag`
- `wdev_branch_wrap_diag`

at the `scan_done_event` stage.

## Result
- Log: [flash_capture_20260319_minimal_rx_recovery_forced_app_scandone_ring/capture.log](../../logs/flash_capture_20260319_minimal_rx_recovery_forced_app_scandone_ring/capture.log)
- This run no longer reached the final `ScanDone` lines within the 120 s window.
- The last visible stage was still `idf_explicit_compare_prestart`.

## Interpretation
This is not a useful narrowing signal.

It means the extra event-handler dump perturbed the already fragile minimal recovery branch enough that the run shape changed again.

## Updated Conclusion
Wrapper-level perturbation is now the limiting factor for further non-hardware narrowing on this branch.

We now have two stable facts:

1. With the reduced wrapper stack, the app can still reach the empty-list `ScanDone` branch, and `wdevProcessRxSucDataAll` still stays unreachable.
2. Adding one more top-half dump at `ScanDone` is already enough to destabilize that branch again.

## Practical Stop Condition
Do not keep adding wrapper-level dumps around the minimal recovery branch.

At this point, further non-hardware progress needs one of:

1. a lower-perturbation binary patch/trampoline above `wdevProcessRxSucDataAll`
2. or waiting for JTAG/debug hardware
