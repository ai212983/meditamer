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
ESPFLASH_PORT=/dev/cu.usbserial-540 FLASH_SET_TIME_AFTER_FLASH=0 scripts/device/flash.sh debug
```

`hostctl flash-capture` defaults:

- `--flash-mode auto`
- `--capture-mode boot`
- `/dev/cu.*` preferred over `/dev/tty.*`
- artifact directory with `flash.log`, `capture.log`, and `summary.txt`

Optional flash env vars consumed by the workflow:

- `ESPFLASH_BAUD` (default `460800` for full flash)
- `ESPFLASH_FALLBACK_BAUD` (default `115200` for app-only fallback)
- `FLASH_TIMEOUT_SEC` (default `360`)
- `ESPFLASH_ENABLE_FALLBACK` (`1` default; app-only fallback via ESP-IDF `esptool.py`)
- `ESPFLASH_SKIP_UPDATE_CHECK` (`1` default)
- `FLASH_SET_TIME_AFTER_FLASH` (`1` default; set `0` to skip automatic `TIMESET`)
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

### C Wi-Fi Control App

Use the official-style ESP-IDF control app when you need to compare this board
against mature C Wi-Fi lifecycle behavior instead of the Rust `esp-radio`
stack:

```bash
scripts/device/wifi_control_idf.sh build
scripts/device/wifi_control_idf.sh flash
scripts/device/wifi_control_idf.sh monitor
```

Behavior:

- default build is scan-only because `CONFIG_WIFI_CONTROL_SSID` defaults empty
- if you later set a non-empty SSID/password in the app config, the same app
  switches to STA-connect mode

ESP-IDF selection:

- wrapper prefers `IDF_APP_ROOT` if set
- otherwise it auto-picks the newest local install under
  `.embuild/espressif/esp-idf/v*`
- for an external install, also export `IDF_TOOLS_PATH` before invoking the
  wrapper so `export.sh` uses the matching toolchain
- the wrapper now auto-resets a stale non-CMake
  `.embuild/idf_apps/wifi_control/build` directory left by failed early runs

Recommended when comparing against the current Wi-Fi blackout:

```bash
export IDF_APP_ROOT="$HOME/.esp-idf/v5.5.2"
export IDF_TOOLS_PATH="$HOME/.espressif"
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/device/wifi_control_idf.sh flash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/device/wifi_control_idf.sh monitor
```

### Wi-Fi Partition Dumps

Use the repo-local helper when debugging Wi-Fi discovery blackout or lower-level
flash state:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/device/dump_wifi_partitions.sh
```

The helper writes a timestamped artifact directory under `logs/flash_dumps/`
and captures:

- `nvs` MD5 and raw dump
- `phy_init` MD5 and raw dump
- first-byte hexdumps for both partitions
- stdout/stderr logs for every `espflash` command
- `summary.txt` with port, baud, read profile, MD5s, and output sizes

Default raw-read transport profile is intentionally conservative because it was
the stable path for `nvs` in blackout debugging:

- `ESPFLASH_BAUD=115200`
- `WIFI_FLASH_DUMP_BLOCK_SIZE=0x100`
- `WIFI_FLASH_DUMP_MAX_IN_FLIGHT=1`

Optional env vars:

- `WIFI_FLASH_DUMP_OUTPUT_ROOT` (default `./logs/flash_dumps`)
- `WIFI_FLASH_DUMP_TIMESTAMP` (default current local timestamp)
- `WIFI_FLASH_DUMP_NVS_ADDRESS` (default `0x9000`)
- `WIFI_FLASH_DUMP_NVS_LENGTH` (default `0x6000`)
- `WIFI_FLASH_DUMP_PHY_INIT_ADDRESS` (default `0xF000`)
- `WIFI_FLASH_DUMP_PHY_INIT_LENGTH` (default `0x1000`)
- `WIFI_FLASH_DUMP_HEXDUMP_BYTES` (default `128`)

Keep using repo-local absolute/anchored paths for artifacts. Do not rely on
wrapper defaults that may execute from `/tmp`.

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


_Runtime setup continues in [Part 07](./part-07.md)._
