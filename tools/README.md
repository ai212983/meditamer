# Developer tools

Use [scripts/](../scripts/README.md) for common repository commands. This folder
holds host implementations, asset compilers and device workflows; test-only
harnesses belong in [test-support/](../test-support/README.md).

## Build and asset production

| Tool | Current purpose |
| --- | --- |
| [event_config_compiler](event_config_compiler/) | Compile the firmware event catalogue from TOML. |
| [lvgl_font_compiler](lvgl_font_compiler/) | Compile LVGL font tables used by firmware builds. |
| [flip_digit_compiler](flip_digit_compiler/) | Compile Flip Clock digit assets. |
| [buzzer_preview](buzzer_preview/) | Compile RTTTL catalogues and preview the shared buzzer sequencer. |
| [ambient_sky](ambient_sky/README.md) | Build and validate ambient-sky packs. |
| [mountain_snow](mountain_snow/README.md) | Generate and pack Mountain Snow assets. |
| [ota_bootloader](ota_bootloader/) | Pinned ESP-IDF bootloader project used by the bootloader wrappers. |

## Operation and validation

| Tool | Current purpose |
| --- | --- |
| [hostctl](hostctl/) | Device operation, flash/capture, upload, update and hardware workflows; launch through `scripts/hostctl.sh`. |
| [touch_replay](touch_replay/README.md) | Import captures and replay deterministic touch fixtures. |
| [hid_capture](hid_capture/README.md) | Capture HID reports and exercise the shared decoder. |
| [analog_clock_preview](analog_clock_preview/) | Preview and export the production Analog Clock renderer. |
| [trace_export](trace_export/README.md) | Convert firmware traces for Perfetto. |
| [refresh-probe](rendering/refresh-probe/README.md) | Compare canonical Rust/reference refresh captures and manually qualify production scan assembly. |

[host-suites.tsv](../scripts/host-suites.tsv) owns host test/lint/coverage
membership; run `scripts/host-test.sh --list` to see the supported suites.
Tool-local helpers stay beside their implementation rather than becoming public
repository commands. Retain tools for production consumers or recurring
qualification; remove completed prototypes and their dedicated tests together.
