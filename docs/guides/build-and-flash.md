# Build, Flash, and Monitor

Day-to-day build, flash, and serial-monitor workflow. Installing a signed
firmware bundle through the factory updater is a separate workflow:
[Firmware Update (ADR-0014)](firmware-update.md).

## Build

```bash
scripts/build/build.sh [debug|release|ble-release|clippy] [default|minimal|slim|telemetry|all-features]
scripts/ci/check_software_baseline.sh [lane]
```

Default is `release` when no argument is provided.

`ble-release` is the size-optimized artifact profile for the non-default
`ble-foundation` fixed-cost probe. It is not a flashing or promotion shortcut;
follow the BLE plan's phase gates before running that artifact on hardware.

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
scripts/device/flash.sh [debug|release|ble-release]
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
- automatic device wall-clock synchronization (`TIMESET`/`TIMEGET` against the
  host's current UTC time and fixed local offset) after every capture branch;
  pass `--no-time-sync` to disable it outright

These bundles accumulate under `logs/` across runs; see
[Log and Artifact Cleanup](development-setup.md#log-and-artifact-cleanup) for the hostctl
commands that inventory and thin them.

Optional flash env vars consumed by the workflow:

- `ESPFLASH_BAUD` (default `460800` for stub-assisted full flash)
- `ESPFLASH_FALLBACK_BAUD` (default `115200` for conservative full or explicit app-only recovery)
- `FLASH_TIMEOUT_SEC` (default `360`)
- `ESPFLASH_ENABLE_FALLBACK` (`1` default; complete ROM-only full-flash fallback)
- `ESPFLASH_SKIP_UPDATE_CHECK` (`1` default)
- `HOSTCTL_FLASH_CAPTURE_BOOT_WINDOW_MS` (default `8000`)
- `HOSTCTL_FLASH_CAPTURE_LOG_PATH` (optional artifact directory override for `scripts/device/flash.sh`)
- `FLASH_SET_TIME_AFTER_FLASH` (`1` default; compatibility override for `hostctl flash-capture`
  directly — set `0` to disable time sync the same way `--no-time-sync` does. `--no-time-sync`
  wins outright when both are present.)

### Wall-clock synchronization

`summary.txt` records the outcome as `time_sync=ok|skipped|failed`, plus
`time_sync_utc`, `time_sync_offset_min`, and `time_sync_reason` (`n/a` when
not applicable). A failed sync does not discard a successful flash: the run
still completes and writes its artifacts, but `hostctl flash-capture` exits
nonzero, summarized as `flash=ok time_sync=failed`. Inspect device time
directly at any point with:

```bash
cargo run --manifest-path tools/hostctl/Cargo.toml -- timestatus
cargo run --manifest-path tools/hostctl/Cargo.toml -- timeset
```

`timestatus` exits successfully whenever it receives and parses a `TIMEGET
OK` response, whether or not the device currently reports a valid time
(`valid=on`/`valid=off`); only a transport error, parse error, or `TIMEGET
ERR` is a failure.

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

- `hostctl flash-capture` first tries complete stub-assisted `esptool.py` flashing at 460800
- when enabled, automatic fallback repeats the complete bootloader, partition, OTA-data, and app write
  at `ESPFLASH_FALLBACK_BAUD` with `--no-stub`; it never changes a full flash into app-only
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

Both full and app-only paths use
[`config/partitions-single-production.csv`](../../config/partitions-single-production.csv).
Full flash also builds and archives the pinned bootloader. App-only recovery resolves the
`ota_0` offset from the CSV rather than assuming a fixed application address.

### BLE Phase 1D baseline

Only run this lane after the BLE plan's durable Phase 1 source gate passes. Give the probe a unique
build identity and retain the canonical flash-capture artifact directory:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 \
HOSTCTL_FLASH_CAPTURE_LOG_PATH=logs/ble_phase1d_flash \
CARGO_FEATURES=ble-foundation \
MEDITAMER_FIRMWARE_BUILD_ID=ble-p1d-001 \
scripts/device/flash.sh ble-release
```

Then run the serialized workflow against that exact artifact set and identify the physical board:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 \
cargo run --manifest-path tools/hostctl/Cargo.toml -- \
  test ble-phase1d \
  --artifacts logs/ble_phase1d_flash \
  --board-id inkplate4-tempera-01
```

The workflow refuses a dirty source identity, an unlabeled build, a non-`ble-release` artifact, a
missing `ble-foundation` feature record, or an ELF/application hash mismatch. It checks HTTP health
before and after the run, requires 20 callback-quiescent controller/host cycles with Wi-Fi owners
resident, and emits a JSON report beside its serial log. A passing report is deliberately a **Phase
1D baseline**, not the full gate: largest-internal-block telemetry and forced closes at both HCI TX
waits and active/full-queue RX ingress remain required by the BLE plan.

## Firmware update

Flashing over USB, above, is the development path. Shipping a new image to a
device that is already in the field goes through the factory updater and a
signed SD-card bundle instead — complete USB flash, bundle build/sign/inspect,
updater status lines, and ROM recovery are all in
[Firmware Update (ADR-0014)](firmware-update.md).

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
