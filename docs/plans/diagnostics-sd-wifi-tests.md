# Diagnostics SD+WiFi Tests Closeout

- Status: Active
- Last-reviewed: 2026-08-14

## Objective

Close the diagnostics work with identified-device evidence that the implemented
combined SD and Wi-Fi session completes in order and releases normal HTTP upload
operation after an explicit exit.

## Implemented source map

| Capability | Current implementation |
| --- | --- |
| Parse `STATE DIAG` and `DIAG GET` | [`serial/parser/basic/state.rs`](../../src/firmware/serial/parser/basic/state.rs), with parser coverage in [`serial/parser/basic/tests.rs`](../../src/firmware/serial/parser/basic/tests.rs) and [`serial/tests/parse_core.rs`](../../src/firmware/serial/tests/parse_core.rs) |
| Enter/leave `DIAGNOSTICS_EXCLUSIVE` and emit start/stop control | [`app_state/machine.rs`](../../src/firmware/app_state/machine.rs) and [`app_state/engine.rs`](../../src/firmware/app_state/engine.rs) |
| Run a bounded diagnostics session and report machine-readable state | [`self_test/control.rs`](../../src/firmware/self_test/control.rs), [`self_test/model.rs`](../../src/firmware/self_test/model.rs), and [`serial/io.rs`](../../src/firmware/serial/io.rs) |
| Execute SD probe then fixed-sector read/write verification | [`self_test/sd_checks.rs`](../../src/firmware/self_test/sd_checks.rs) |
| Check Wi-Fi upload mode and connected-link readiness | [`self_test/wifi.rs`](../../src/firmware/self_test/wifi.rs) |
| Run combined targets deterministically as SD then Wi-Fi | [`self_test/control.rs`](../../src/firmware/self_test/control.rs) |
| Pause HTTP transfers during diagnostics and resume after exit | [`service_mode.rs`](../../src/firmware/service_mode.rs) and [`storage/upload/http/server_loop.rs`](../../src/firmware/storage/upload/http/server_loop.rs) |

The UART command and response formats are documented in
[`guides/service-modes.md`](../guides/service-modes.md). Display, touch, and IMU
targets remain unsupported by this diagnostics task and are outside this plan.

## Remaining identified-device acceptance gate

This gate is still pending; this plan does not claim that it has run.

1. Record the exact firmware artifact identity and the physical device/serial
   port used. Ensure the device has working Wi-Fi credentials and upload mode is
   enabled.
2. Send `STATE DIAG kind=TEST targets=SD|WIFI`, then use `STATE GET` to require
   `phase=DIAGNOSTICS_EXCLUSIVE`, `diag_kind=TEST`, and `targets=SD|WIFI`.
3. Poll `DIAG GET` and retain the UART transcript. Require successful progression
   through the SD steps (`sd_probe`, then `sd_rwverify`) before the Wi-Fi step
   (`wifi_ready`), followed by `state=done step=complete code=0` for
   `targets=SD|WIFI`. Any failed, canceled, reversed, or incomplete sequence
   fails the gate.
4. Send `STATE DIAG kind=NONE targets=NONE`; require `STATE GET` to report the
   normal `phase=OPERATING` state with `diag_kind=NONE` and `targets=NONE`, and require
   `DIAG GET` to return to `state=idle targets=NONE step=idle code=0`.
5. Without rebooting, run a normal HTTP asset upload using the workflow in
   [`guides/wifi-asset-upload.md`](../guides/wifi-asset-upload.md) and require it
   to succeed on the same device. Retain the command result with the UART
   transcript.

When this single gate passes, change the plan status to `Done` and link the
retained evidence from the appropriate live reference or guide.
