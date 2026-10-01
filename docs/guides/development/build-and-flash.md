# Build, flash, and monitor

Run commands from the repository root. Set the connected serial port explicitly
when more than one device is attached; examples use `$DEVICE_PORT`.
Signed SD-card updates are a separate [workflow](firmware-update.md).

## Build

```bash
scripts/build/build.sh [debug|release] [default|minimal]
scripts/ci/clippy-firmware.sh [debug|release] [default|minimal]
```

Both default to `release default`. Clippy produces no firmware image. The wrappers
share target/toolchain/linker selection and keep Inkplate artifacts under root
`target/`. The [feature catalogue](../../references/compile-time-features.md) owns
compositions; the [setup guide](setup.md#software-checks) owns
software baseline lanes.

| Selection | Example |
| --- | --- |
| Production | `scripts/build/build.sh release default` |
| Minimal, without optional default services | `scripts/build/build.sh debug minimal` |
| Extra diagnostic features | `CARGO_FEATURES=telemetry-defmt scripts/build/build.sh debug default` |
| Standalone binary | `FIRMWARE_BIN=<probe> CARGO_FEATURES=<required-feature> scripts/build/build.sh release minimal` |

Builds use `--locked`; set `CARGO_LOCKED=0` only to update a lockfile deliberately.
`CARGO_FEATURES` adds features; `CARGO_NO_DEFAULT_FEATURES=1` disables defaults.
The same selectors apply to Clippy. Restore production after
[fixtures](../diagnostics/inkplate-fixtures.md) or [buzzer probes](../audio/buzzer-listening.md).

## Inkplate flash

```bash
ESPFLASH_PORT="$DEVICE_PORT" scripts/device/flash.sh release
```

This builds and flashes the complete single-production layout, boots production,
and captures startup plus host/device time synchronization. It defaults to
`release`. Use a new artifact directory to identify the candidate:

```bash
ESPFLASH_PORT="$DEVICE_PORT" MEDITAMER_FIRMWARE_BUILD_ID=<build-id> HOSTCTL_FLASH_CAPTURE_LOG_PATH=logs/<new-flash-run> scripts/device/flash.sh release
```

For factory/updater qualification on a device with its updater already installed:

```bash
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh flash-capture \
  --profile release --boot-target factory --log logs/<new-factory-run>
```

Direct `flash-capture` defaults to factory; the wrapper selects production.
Install the factory updater initially using [complete USB flash](firmware-update.md#complete-usb-flash);
ordinary flashing preserves the installed updater. Select app-only recovery
explicitly when preserving OTA boot selection:

```bash
ESPFLASH_PORT="$DEVICE_PORT" HOSTCTL_FLASH_CAPTURE_FLASH_MODE=app-only ESPFLASH_FALLBACK_BAUD=115200 scripts/device/flash.sh debug
```

Keep the capture connection open: reopening/resetting an unconfirmed production
candidate can trigger rollback to factory. Inspect the [flash artifacts and
outcome fields](../../references/hostctl.md#flash-capture) before retrying a failed run.

## Medinote / Waveshare flash

```bash
targets/medinote-waveshare/build.sh --locked
ESPFLASH_PORT="$DEVICE_PORT" targets/medinote-waveshare/flash.sh
```

The wrapper loads `~/export-esp.sh` and derives compiler/Bindgen paths; source it
manually only for direct Cargo commands. This target owns its ESP32-S3 partition
and image layout and does not reuse Inkplate bootloader/OTA handling.
Flash and boot evidence go to `logs/medinote_flash_*`, including ELF hash,
commit/dirty identity, and verified clock fields. `MEDINOTE_BOOT_WINDOW_SEC`
changes the default 35-second capture. Missing/failed sync preserves artifacts
and exits nonzero. Wi-Fi requires an explicit [feature selection](../network/asset-upload.md).

## Serial ports and recovery

```bash
ls -1 /dev/cu.* /dev/tty.* 2>/dev/null
espflash board-info -p "$DEVICE_PORT" -c esp32
lsof "$DEVICE_PORT"
```

Prefer `/dev/cu.*` on macOS. Auto-selection requires one unambiguous candidate;
`ESPFLASH_PORT_HINT` narrows the match. Stop any monitor/holder before flashing.
For ROM recovery, use a complete [USB flash](firmware-update.md#complete-usb-flash).

## Monitor

```bash
ESPFLASH_PORT="$DEVICE_PORT" scripts/device/monitor.sh
```

Default passive attach preserves DTR/RTS. On Inkplate, USB-UART wiring reaches
GPIO0, also the panel CL clock; `espflash monitor` can disturb the display.
Use flash-capture for early boot evidence; see [monitor controls](../../references/hostctl.md#monitor)
for mode and transport overrides.
With `tio`, exit using `Ctrl+T`, then `q`.

## Defmt

```bash
ESPFLASH_PORT="$DEVICE_PORT" CARGO_FEATURES=telemetry-defmt scripts/device/flash.sh debug
ESPFLASH_PORT="$DEVICE_PORT" ESPFLASH_MONITOR_MODE=espflash scripts/device/monitor.sh
```

Only `espflash` mode decodes defmt. Its attach can disturb Inkplate GPIO0/CL
until a full refresh; passive/raw modes do not decode these frames.

## Manual clock synchronization

```bash
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh time-status
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh time-set
```

Inspect clock validity using the [synchronization result semantics](../../references/hostctl.md#clock-synchronization).
