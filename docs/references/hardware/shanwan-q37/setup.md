# Q37 setup

Last verified: 2026-08-28, using macOS host tools.
Tested identity: `ShanWan BM-769`, VID/PID `1949:0402`, firmware string
`1.00.00:1.00.00`. The user updated the firmware; this string may not distinguish
versions.

## Connection modes

| Advertised name | Entry from power-off / purpose | Support |
| --- | --- | --- |
| `GamepadSpace-Q37` | Hold L + X + Home; manual calls this firmware-upgrade mode | BLE gamepad input and rumble verified; use this connection |
| `ShanWan Q37XSP` | Hold R + X + Home; ShootingPlus touchscreen mode | Touch reports, unsupported by the gamepad decoder |
| `XBOX Wireless Controller` | Normal X-INPUT mode, switch at X | Classic Bluetooth according to the user; outside BLE scope |

The physical X/P/S switch is separate from the special BLE connection mode.

## GATT services

Discover characteristics by UUID, not captured handles. Vendor UUIDs below use
suffix `-1111-6666-8888-0123456789ab`; standard short UUIDs use the Bluetooth base.

| Service | Characteristics | Purpose |
| --- | --- | --- |
| `180A` | Read `2A29`, `2A26`, `2A50` | Manufacturer, firmware, PnP identity (`01 49 19 02 04 00 00`) |
| `1812` | HID reports | Hidden by macOS CoreBluetooth; accessible through HIDAPI |
| `95990001` | Write `95990002`, notify `95990003` | Vendor input, configuration and rumble |
| `91680001` | Write `91680002`, notify `91680003` | Not needed for the selected input path |
| `FFFF` | Notify `FF11`, write `FF22` | Firmware update; do not probe with arbitrary writes |

Direct HID GATT Report References and notification framing remain unverified.
Do not assume HIDAPI's leading report ID is present in a GATT notification.

## Gyro configuration

The official GamepadSpace app can bind gyro to the left stick, right stick, or
none. Use it to disable gyro-to-stick mapping before using Motion for physical
stick input. With the user's tested app setup, tilting no longer changed Motion
axes, both physical sticks worked, and Home/O remained available, including after O.

Host gyro configuration remains unresolved.
