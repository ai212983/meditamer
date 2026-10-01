# Central input and device support

Status: Active

Provide shared Q37 and CheerTok input/feedback drivers for both products.
Build on the [shared BLE mechanisms](../../../platform/connectivity/ble/src/lib.rs)
for connections and security.

## Work

- Expose a driver-facing connection/session API with GATT access and stale-session rejection.
- Persist bonds in target flash and add forget/re-pair. Reconnect must work without SD.
- Complete retry/reconnect and integrate central input on Inkplate; preserve
  Medinote stop/resume through the [radio policy](radio-policy-and-qualification.md).
- Define common capabilities, named controls, normalized axes, held state and
  press/release edges. Keep device decoding in drivers and action bindings in products.
- Implement [Q37 Motion input, Home/O and bounded rumble](../../references/hardware/shanwan-q37/README.md);
  adapt [CheerTok controls](../../references/hardware/cheertok/input.md) to the same interface.
  Route feedback to the current session and cancel held input on disconnect.

Driver work and capture replay can proceed before bond persistence.

## Checks

- Test session recovery, stale work, bond failures, input decoding and edge/overflow handling.
  Use independently sourced descriptors and expected results for generic HID coverage.
- On each target verify supported controls, Q37 Motion/rumble interaction and link-loss
  behavior, reconnect, forget/re-pair and saved bonds after reboot, with and without SD.
- Pass [integrated qualification](radio-policy-and-qualification.md#qualification).
