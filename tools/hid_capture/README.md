# Host HID capture

Capture a connected controller's descriptor and raw reports, then replay them
through `platform/connectivity/ble` on the host. No firmware build or flash is involved.
On macOS, this uses HIDAPI with shared device access; it does not seize the device.
Allow any macOS input-monitoring permission request for the host application.

From the repository root:

```sh
cargo +stable build --manifest-path tools/hid_capture/Cargo.toml --locked
tools/hid_capture/target/debug/hid_capture list GamepadSpace
mkdir -p tools/hid_capture/out
tools/hid_capture/target/debug/hid_capture capture 'GamepadSpace-Q37' 120 tools/hid_capture/out/capture.jsonl 1:5
tools/hid_capture/target/debug/hid_capture replay tools/hid_capture/out/capture.jsonl
```

Connect the controller through macOS Bluetooth settings first. Use the exact name
from `list`; capture refuses ambiguous matches and existing output files.
The optional `HEX-PAGE:HEX-USAGE` selector chooses a collection: `1:5` selects
Game Pad when the same device also exposes consumer-control and pointer collections.
During capture, release all controls, press each button separately, exercise the
D-pad and full stick/trigger travel, then try simultaneous presses.

The JSONL header contains the device identity and unmodified HID descriptor.
Input records contain elapsed microseconds and HIDAPI bytes: numbered reports
include the report ID; unnumbered reports do not have an extra zero byte.
The end record gives the report count. Interrupted captures retain flushed records.
Ctrl-C stops capture early; replay then reports `capture_complete=false` because
there is no end record. This does not discard the reports already saved.
Capture preserves reports beyond the firmware decoder's limits; replay reports
unsupported layouts instead of modifying the raw evidence.

Replay prints each decoded report independently, without axis scaling or merging
state across report IDs. A successful replay is decoder evidence, not confirmation
of gamepad semantics, GATT Report References, pairing, or Waveshare timing.
The controller's selected mode must be recorded alongside captures.

## Q37 reference

See the [Q37 index](../../docs/references/hardware/shanwan-q37/README.md) for setup,
input layouts and rumble. The [connection modes](../../docs/references/hardware/shanwan-q37/setup.md#connection-modes)
distinguish supported `GamepadSpace-Q37` input from unsupported Q37XSP touch reports.

## Checks

```sh
cargo +stable test --manifest-path tools/hid_capture/Cargo.toml --locked
cargo +stable clippy --manifest-path tools/hid_capture/Cargo.toml --locked --all-targets -- -D warnings
scripts/host-test.sh test ble
python3 -m unittest discover -s tools/hid_capture -p 'test_q37_rumble.py' -v
```

## Q37 BLE rumble (macOS)

Connect `GamepadSpace-Q37` in macOS Bluetooth settings first. The separate
[Python command](q37_rumble.py) uses the verified vendor GATT motor-test command;
it does not need a firmware flash or HID Output report. Left and right channels
were physically confirmed independently on the captured unit.

Install the tested dependency in the ignored output directory:

```sh
python3 -m venv tools/hid_capture/out/venv
tools/hid_capture/out/venv/bin/pip install bleak==3.0.1
tools/hid_capture/out/venv/bin/python tools/hid_capture/q37_rumble.py left --duration-ms 250
tools/hid_capture/out/venv/bin/python tools/hid_capture/q37_rumble.py right --duration-ms 250
tools/hid_capture/out/venv/bin/python tools/hid_capture/q37_rumble.py both --duration-ms 250
tools/hid_capture/out/venv/bin/python tools/hid_capture/q37_rumble.py stop
```

Pulses default to 250 ms and accept 1–1000 ms. Both motors receive stop commands
before and after a pulse, including normal Ctrl-C cleanup. Use `--output NEW.jsonl`
to save the event log instead of printing it. Existing files are preserved.
The tool checks the controller name, manufacturer and PnP identity before writing.

Write completion is not a device acknowledgment. Link loss can prevent stop;
if vibration continues, turn the controller off with its rear Pair/Power button.
These commands select channels on/off; adjustable intensity is not established.
