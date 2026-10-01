# CheerTok input

Device-specific captured observations. Shared contracts live in the
[BLE modules](../../../../platform/connectivity/ble/src/lib.rs);
remaining driver integration in the [device plan](../../../plans/ble/central-input.md).

## Connection and reports

CheerTok requires security before GATT discovery. Its captured Report Map is
345 bytes. Use Report References to route notifications: the report ID can be
separate from the GATT payload.

| Report ID | Kind |
| --- | --- |
| 1 | Keyboard input/output |
| 2 | Consumer input |
| 3 | Mouse input |
| 5 | Touch input |
| 7 | Pen input |

Mouse ID 3 has a seven-byte payload: button bits in byte 0, signed wheel in byte 1,
packed signed 12-bit relative X/Y in bytes 2–4, signed AC Pan in byte 5 and AC Zoom
in byte 6.

## Observed gestures

| Gesture | Reported control |
| --- | --- |
| One-finger movement/tap | Pointer movement / left click |
| Two-finger tap | Right click |
| Two-finger vertical/horizontal movement | Wheel / AC Pan |
| Pinch in/out | Left Ctrl with negative/positive wheel |
| Upper/side edge swipes | Page Up for left/up; Page Down for right/down |

Tested bottom-edge swipes, three-finger taps, hold/drag gestures and the laser
button produced no usable reports; this describes the observed capture coverage.
Medinote's [product mapping](../../../../products/medinote/src/controls.rs) consumes
only Page Down/Up for clockwise/counterclockwise rotation through the shared
state/edge publisher. This does not establish support for the other captured controls.
