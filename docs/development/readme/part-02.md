## Flash

Flash (auto-detects serial port when exactly one candidate is present):

```bash
scripts/device/flash.sh [debug|release]
```

Default is `release` when no argument is provided.

Recommended explicit invocation (best for multi-device setups):

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 FLASH_SET_TIME_AFTER_FLASH=0 scripts/device/flash.sh debug
```

Optional flash env vars:

- `ESPFLASH_BAUD` (default `460800` in `scripts/device/flash.sh`)
- `FLASH_TIMEOUT_SEC` (default `360`; watchdog timeout per primary flash attempt)
- `FLASH_STATUS_INTERVAL_SEC` (default `15`; heartbeat interval while flashing)
- `ESPFLASH_ENABLE_FALLBACK` (`1` default; retries with `--no-stub` on failure/timeout)
- `ESPFLASH_FALLBACK_BAUD` (default `115200`)
- `ESPFLASH_SKIP_UPDATE_CHECK` (`1` default; avoids crates.io version-check delay)
- `FLASH_SET_TIME_AFTER_FLASH` (`1` default; set `0` to skip automatic `TIMESET`)

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

- `scripts/device/flash.sh` now prints `Flashing in progress...` every `FLASH_STATUS_INTERVAL_SEC` seconds.
- A flash watchdog aborts after `FLASH_TIMEOUT_SEC`; with fallback enabled, it retries automatically using `--no-stub`.

If serial port is busy:

```bash
lsof /dev/cu.usbserial-540
```

Stop monitor/holder processes, then re-run flash.

Force slow fallback path directly:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 ESPFLASH_BAUD=115200 ESPFLASH_ENABLE_FALLBACK=0 scripts/device/flash.sh debug
```

## Monitor

```bash
scripts/device/monitor.sh
```

Optional monitor env vars:

- `ESPFLASH_BAUD` (default `115200`)
- `ESPFLASH_MONITOR_BEFORE` (default `default-reset`)
- `ESPFLASH_MONITOR_AFTER` (default `hard-reset`)
- `ESPFLASH_MONITOR_MODE` (`espflash` default, `raw` for direct serial read without reset/sync)
- `ESPFLASH_MONITOR_PERSIST_RAW` (`1` default: keep raw monitor alive across unplug/replug, `0` to exit on disconnect)
- `ESPFLASH_MONITOR_RAW_BACKEND` (`auto` default; `tio` preferred if installed, fallback `cat`)
- `ESPFLASH_MONITOR_OUTPUT_MODE` (`normal` default; `hex` can help diagnose garbled UART output)

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

## Time Sync

Firmware accepts a UART command on `UART0` (`115200` baud):

```text
TIMESET <unix_epoch_utc_seconds> <tz_offset_minutes>
```

Examples:

- `TIMESET 1762531200 -300` (UTC-05:00)
- `TIMESET 1762531200 60` (UTC+01:00)

Recommended host helper:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/device/timeset.sh
```

Optional explicit values:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/device/timeset.sh 1762531200 -300
```

If you prefer manual write:

```bash
stty -f /dev/cu.usbserial-540 115200 cs8 -cstopb -parenb -ixon -ixoff -crtscts -echo raw
printf 'TIMESET %s %s\r\n' "$(date -u +%s)" "-300" > /dev/cu.usbserial-540
```

## Allocator Diagnostics

Firmware accepts allocator status commands on `UART0` (`115200` baud):

```text
PSRAM
```

Aliases: `HEAP`, `ALLOCATOR`.

Response format:

```text
PSRAM feature_enabled=<bool> state=<state> total_bytes=<n> used_bytes=<n> free_bytes=<n> peak_used_bytes=<n> internal_free_bytes=<n> external_free_bytes=<n> min_free_bytes=<n> min_internal_free_bytes=<n> min_external_free_bytes=<n> large_alloc_external_ok=<n> large_alloc_internal_ok=<n> large_alloc_fail=<n>
```

- `internal_free_bytes` tracks capability-constrained internal RAM available for Wi-Fi/radio allocations.
- `min_*` values are boot-lifetime low-water marks to identify monotonic pressure during soak runs.
- `large_alloc_*` counters show where `alloc_large_byte_buffer` requests landed (external vs internal fallback).

Allocator probe command:

```text
PSRAMALLOC <bytes>
```

Alias: `HEAPALLOC <bytes>`.

Probe responses:

```text
PSRAMALLOC OK bytes=<n> placement=<placement> len=<n>
PSRAMALLOC ERR bytes=<n> reason=<reason>
```

## Runtime Service Modes

Runtime mode controls are available over `UART0` (`115200` baud):

```text
STATE GET
STATE SET upload=on
STATE SET upload=off
STATE SET assets=on
STATE SET assets=off
STATE SET base=day
STATE SET base=touch_wizard
STATE SET day_bg=suminagashi
STATE SET day_bg=shanshui
STATE SET overlay=none
STATE SET overlay=clock
STATE DIAG kind=debug targets=SD|WIFI
DIAG GET
```

Response format:

```text
STATE phase=<...> base=<...> day_bg=<...> overlay=<...> upload=<on|off> assets=<on|off> diag_kind=<...> targets=<NONE|SD|WIFI|DISPLAY|TOUCH|IMU>
DIAG state=<idle|running|done|failed|canceled> targets=<...> step=<...> code=<...>
```

Notes:

- App state is persisted in flash and restored on boot.
- `STATE SET` returns `OK` only after the state update is applied by runtime tasks.
- `STATE SET upload=off` rejects upload operations and releases upload transfer buffers.
- `STATE SET assets=off` disables SD asset reads, clears runtime graphics cache, and releases asset-read transfer buffers.
- On `psram-alloc` builds, transfer buffers are allocated in PSRAM on-demand and released when the mode is disabled.

Quick RAM check sequence:

```text
PSRAM
STATE SET upload=on
PSRAM
STATE SET upload=off
PSRAM
STATE SET assets=off
PSRAM
STATE SET assets=on
PSRAM
```

Automated smoke run (mode toggles + PSRAM snapshots):

```bash
scripts/device/runtime_modes_smoke.sh
```

Optional env var:

- `HOSTCTL_MODE_SMOKE_SETTLE_MS` (default `0`; can be raised if extra post-command delay is desired)

