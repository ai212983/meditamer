# Build, Flash, and Monitor

## Build

```bash
scripts/build/build.sh [debug|release|clippy] [default|minimal|slim|telemetry|all-features]
scripts/ci/check_software_baseline.sh [lane]
```

Default is `release` when no argument is provided.

See [Compile-Time Features](../reference/compile-time-features.md) for the supported
feature profiles and the functionality that is now unconditional.

The default Xtensa runner (`scripts/build/xtensa_runner.sh`) flashes firmware without opening
an interactive monitor (safe in non-interactive shells). To enable monitor explicitly:

```bash
ESPFLASH_RUN_MONITOR=1 cargo +esp run -Zbuild-std=core,alloc --target xtensa-esp32-none-elf
```

## Flash

Canonical flash and boot-capture entrypoint:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 \
RUSTUP_TOOLCHAIN=stable \
cargo run --manifest-path tools/hostctl/Cargo.toml --target "$(rustup run stable rustc -vV | awk '/^host:/ {print $2}')" -- \
  flash-capture \
  --profile debug \
  --log logs/flash_capture_manual
```

Compatibility wrapper:

```bash
scripts/device/flash.sh [debug|release]
```

Default is `release` when no argument is provided.

Recommended explicit wrapper invocation:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/device/flash.sh debug
```

`hostctl flash-capture` defaults:

- `--flash-mode auto`
- `--capture-mode boot`
- `/dev/cu.*` preferred over `/dev/tty.*`
- artifact directory with `flash.log`, `capture.log`, and `summary.txt`

Optional flash env vars consumed by the workflow:

- `ESPFLASH_BAUD` (default `115200` for full flash)
- `ESPFLASH_FALLBACK_BAUD` (default `115200` for app-only fallback)
- `FLASH_TIMEOUT_SEC` (default `360`)
- `ESPFLASH_ENABLE_FALLBACK` (`1` default; app-only fallback via ESP-IDF `esptool.py`)
- `ESPFLASH_SKIP_UPDATE_CHECK` (`1` default)
- `HOSTCTL_FLASH_CAPTURE_BOOT_WINDOW_MS` (default `8000`)
- `HOSTCTL_FLASH_CAPTURE_LOG_PATH` (optional artifact directory override for `scripts/device/flash.sh`)

### Port Selection

Hardware scripts now auto-detect a port when exactly one candidate is available.
Use explicit `ESPFLASH_PORT` in multi-device or CI/non-interactive setups.
A known-good port on this setup is:

- `/dev/cu.usbserial-540`

List available serial ports:

```bash
ls -1 /dev/cu.* /dev/tty.* 2>/dev/null
```

Verify board connection on a specific port:

```bash
espflash board-info -p /dev/cu.usbserial-540 -c esp32
```

If autodetection is ambiguous, set `ESPFLASH_PORT` explicitly.
You can also narrow autodetection with `ESPFLASH_PORT_HINT` (substring match).

### Flash Troubleshooting

If flashing appears "stuck":

- `hostctl flash-capture` first tries full `espflash` and then falls back to app-only `esptool.py` when enabled.
- the fallback path uses the ESP-IDF virtualenv Python and avoids `save-image --merge`
- the wrapper preserves the old `FLASH_TIMEOUT_SEC` and fallback env knobs

If serial port is busy:

```bash
lsof /dev/cu.usbserial-540
```

Stop monitor/holder processes, then re-run flash.

Force app-only fallback directly:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 \
HOSTCTL_FLASH_CAPTURE_FLASH_MODE=app-only \
ESPFLASH_FALLBACK_BAUD=115200 \
scripts/device/flash.sh debug
```

## Monitor

```bash
scripts/device/monitor.sh
```

Optional monitor env vars:

- `ESPFLASH_BAUD` (default `115200`)
- `ESPFLASH_MONITOR_BEFORE` (default `no-reset-no-sync`)
- `ESPFLASH_MONITOR_AFTER` (default `no-reset`)
- `ESPFLASH_MONITOR_MODE` (`espflash` default, `raw` for direct serial read without reset/sync)
- `ESPFLASH_MONITOR_PERSIST_RAW` (`1` default: keep raw monitor alive across unplug/replug, `0` to exit on disconnect)
- `ESPFLASH_MONITOR_RAW_BACKEND` (`auto` default; `tio` preferred if installed, fallback `cat`)
- `ESPFLASH_MONITOR_OUTPUT_MODE` (`normal` default; `hex` can help diagnose garbled UART output)

`scripts/device/monitor.sh` is now the passive attach/debug helper. Use
`hostctl flash-capture` for boot-phase capture; do not rely on `espflash monitor`
reset sequences for early boot logging on this board.

When raw backend is `tio`, exit the monitor with `Ctrl+T` then `q`.

For boards without reset wiring/button, prefer raw mode:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 ESPFLASH_MONITOR_MODE=raw scripts/device/monitor.sh
```

### Defmt Telemetry

Firmware supports optional `defmt` telemetry via feature `telemetry-defmt`.

Build/flash with defmt telemetry enabled:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 CARGO_FEATURES=telemetry-defmt scripts/device/flash.sh debug
```

Use espflash monitor mode (not raw cat/tio) to decode defmt frames:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 ESPFLASH_MONITOR_MODE=espflash scripts/device/monitor.sh
```

Raw monitor mode (`ESPFLASH_MONITOR_MODE=raw`) does not decode defmt frames.

