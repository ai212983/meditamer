# Q37 BLE rumble

Last verified: 2026-08-28. Both motors and independent left/right channels were
physically confirmed on `GamepadSpace-Q37` over BLE.

Write five bytes **without response** to
`95990002-1111-6666-8888-0123456789ab`, in service
`95990001-1111-6666-8888-0123456789ab`:

| Action | Bytes |
| --- | --- |
| Left only | `10 05 3e ff 00` |
| Right only | `10 05 3e 00 ff` |
| Both | `10 05 3e ff ff` |
| Stop both | `10 05 3e 00 00` |

These commands select channels on/off. Adjustable intensity is unverified.
No initialization or strength-setting write was needed.

Rumble worked alongside changing stick input in **HID** and **Gamepad**.
Coexistence with **Motion** still needs verification.

Use bounded pulses and send stop before and after, including cancellation cleanup.
Write completion is not a device acknowledgment; link loss can prevent stop.
Link-loss behavior remains unverified. If vibration continues, turn the controller
off using the rear Pair/Power button.
