# Q37 input

Last verified: 2026-08-28. These three input paths belong to the
[`GamepadSpace-Q37` BLE connection](setup.md); they are not the physical X/P/S modes.
Motion and Gamepad are project names for vendor polling selectors.

## Available paths

| Path | Activation | Controls | Home / O |
| --- | --- | --- | --- |
| **HID** | Ordinary HID subscription | Standard buttons, D-pad, sticks and triggers verified | Not exposed in observed reports |
| **Motion** | Poll `10 04 07 03` | Standard controls and physical sticks verified with the app gyro setup | Both available |
| **Gamepad** | Poll `10 04 07 02` | Physical sticks and A verified without tilt effects; remaining controls need qualification | Not exposed in observed reports |

Use **Motion** with the [app gyro configuration](setup.md#gyro-configuration);
Gamepad is the fallback. Vendor polling paused ordinary HID during testing.

## Vendor reports

Subscribe to `95990003…` and write the selected poll command **without response**
to `95990002…` every 500 ms. Notifications carry 20-byte input snapshots.

| Bytes | Meaning |
| --- | --- |
| 0–1 | `11 14` or `11 20`; both occur with Motion-only polling |
| 2 | Observed `0f`; not the D-pad |
| 3–6 | LX, LY, RX, RY; range 0–255, observed rest 128; left/up decrease values |
| 7–8 | L2, R2; only 0/255 observed, continuous analog travel unverified |
| 9–19 | Active key codes, zero-padded; slots can move |

Decode both prefixes from the full body. A prefix is **not a source identifier**:
Motion can send active input as `11 14` and neutral/release input as `11 20`.
Compare key sets for press/release edges. Unchanged input can be silent, so absence
of periodic reports alone is not a release.

## Control map

Vendor codes are verified for Motion; HID values are hexadecimal button masks.

| Controls | Vendor codes | HID report 4 |
| --- | --- | --- |
| A, B, X, Y | `a6`, `a4`, `a2`, `a0` | `0001`, `0002`, `0008`, `0010` |
| L1, R1 | `9e`, `9a` | `0040`, `0080` |
| L2, R2 | `9c`, `98` | `0100`, `0200` |
| View (-), Menu (+) | `88`, `8a` | `0400`, `0800` |
| L3, R3 | `96`, `94` | `2000`, `4000` |
| Home, O | `86`, `83` | Not observed |
| D-pad Up, Right, Down, Left | `ae`, `a8`, `ac`, `aa` | Hat 0, 2, 4, 6 |

O also emits companion code `84`; use `83` for O and do not emit a second press.
Diagonals and tested combinations retain independently held keys. Maximum
simultaneous-key capacity and long-press behavior are not established.

## HID report 4

HIDAPI supplies eleven bytes including the report ID:

| Bytes | Meaning |
| --- | --- |
| 0 | Report ID `04` |
| 1–4 | LX, LY, RX, RY |
| 5, low nibble | Hat: 0=up, clockwise through 7=up-left; 15=neutral |
| 6–7 | Little-endian button mask from the table above |
| 8–9 | R2, L2; only 0/255 observed |
| 5, high nibble; 10 | Padding; ignore |

The [shared decoder](../../../../platform/connectivity/ble/src/hid.rs) returns raw axes in
descriptor order: X, Y, Z, Rz, Accelerator, Brake. The
[descriptor](../../../../platform/connectivity/ble/tests/fixtures/q37-gamepad-descriptor.bin) and
[HID fixtures](../../../../tools/hid_capture/fixtures/) support host replay.
Consumer report 3 is declared but was not observed; no HID Output/Feature reports
are declared. Rumble uses the [vendor command](rumble.md).

## Stick handling

Use raw values before deciding on a small dead zone or explicit calibration.
Resting sticks can wobble. Never learn neutral during normal use, when a stick may
be held, or treat gyro-induced movement as drift. Preserve full travel when scaling.
HID tilt behavior also depends on app settings; its physical-stick response under
the latest setup needs qualification if selected as a fallback.
