# Build, Flash, and Monitor

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
  at `ESPFLASH_FALLBACK_BAUD` with `--no-stub`; it never changes an A/B full flash into app-only
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

Both full and app-only paths use [`config/partitions-ab.csv`](../../config/partitions-ab.csv).
Full flash also builds and archives the pinned rollback bootloader. App-only recovery resolves the
`ota_0` offset from the CSV; it does not assume the former `0x10000` application address.

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

## Signed A/B firmware update

Generate a private 32-byte Ed25519 seed outside tracked source, then derive the public build value:

```bash
scripts/device/generate_firmware_signing_key.sh target/firmware-signing.seed
cargo run --manifest-path tools/hostctl/Cargo.toml -- firmware-key \
  --key target/firmware-signing.seed
```

Keep the seed private and backed up. The generator refuses to overwrite an existing key. Put the
printed public value and an observable build id into the release build:

```bash
MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX=<64-hex-public-key> \
MEDITAMER_FIRMWARE_BUILD_ID=<1-31-character-id> \
scripts/build/build.sh release

espflash save-image --chip esp32 \
  --partition-table config/partitions-ab.csv \
  --target-app-partition ota_0 \
  target/xtensa-esp32-none-elf/release/meditamer \
  target/meditamer-update.bin
```

The first A/B installation is a full flash using the same public-key environment. It migrates the
legacy lifecycle record before any update may touch the second slot:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 \
HOSTCTL_FLASH_CAPTURE_FLASH_MODE=full \
MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX=<64-hex-public-key> \
MEDITAMER_FIRMWARE_BUILD_ID=<build-id> \
scripts/device/flash.sh release
```

Stage, verify, activate, boot, and confirm a signed application binary:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 \
HOSTCTL_FIRMWARE_UPDATE_LOG_PATH=logs/firmware-update.log \
scripts/hostctl.sh firmware-update \
  --image target/meditamer-update.bin \
  --key target/firmware-signing.seed
```

`scripts/hostctl.sh` is the direct launcher. Relative typed paths and default evidence paths are
resolved from the repository root even though its isolated Cargo process starts in `/tmp`.

Set `HOSTCTL_FIRMWARE_UPDATE_STAGE_ONLY=1` to stop after signature and full flash read-back
verification, before OTA metadata activation. `FWSTATUS` reports the running build/slot, selection
state, key id, staging progress, maximum erase/write call times, read-back time, and multicore flash
policy. A missing compiled public key fails closed for update staging but does not prevent normal boot.

A missing chunk acknowledgement is retried at most twice with the exact same offset and payload;
firmware recognizes only that immediately previous chunk as an idempotent resend. Explicit chunk
errors and an ambiguous activation acknowledgement are never retried. Recover by checking
`FWSTATUS`; use the full-flash path if neither application slot remains usable.

Firmware that advertises `stream=bin1@460800` uses CRC-framed 112-byte binary payloads. Each complete
126-byte frame fits UART0's 128-byte FIFO, and two payloads coalesce into one 224-byte internal-RAM
flash batch. Older firmware transparently retains the 48-byte hex transport for the first upgrade.
The inactive image range is erased before binary streaming; interruption still leaves the running
slot selected. On a binary transport failure, the host restores 115200 and resets the board without
activating the partial image.

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
