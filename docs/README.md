# meditamer

## Runtime Stack

The firmware now targets `esp-hal` (`xtensa-esp32-none-elf`) as the primary and only runtime path.

## Event Engine Plan

- `docs/development/statig-event-engine-plan.md`: phased migration plan for replacing in-task heuristic tap logic with a `statig`-based generic sensor-event engine.
- `docs/development/event-engine-guide.md`: practical developer guide for tuning and modifying the current event engine implementation.

## Display Runtime Behavior

- Clock refresh task: every 5 minutes (`300s`)
- Battery task: independent Embassy task every 5 minutes (`300s`)
- Battery label: top-right (`BAT xx%`)
- Battery percentage source: BQ27441 fuel gauge `SoC` register (reference behavior)

## Build

```bash
scripts/build.sh [debug|release]
```

Default is `release` when no argument is provided.

## Flash

Set the serial port and flash:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/flash.sh [debug|release]
```

Default is `release` when no argument is provided.

### Port Selection

Use an explicit port in non-interactive environments. A known-good port on this setup is:

- `/dev/cu.usbserial-540`

List available serial ports:

```bash
ls -1 /dev/cu.* /dev/tty.* 2>/dev/null
```

Verify board connection on a specific port:

```bash
espflash board-info -p /dev/cu.usbserial-540 -c esp32
```

If `espflash` reports `IO error: not a terminal`, set `ESPFLASH_PORT` (or pass `-p`) to avoid interactive port selection.

## Monitor

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/monitor.sh
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
ESPFLASH_PORT=/dev/cu.usbserial-540 ESPFLASH_MONITOR_MODE=raw scripts/monitor.sh
```

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
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/timeset.sh
```

Optional explicit values:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/timeset.sh 1762531200 -300
```

If you prefer manual write:

```bash
stty -f /dev/cu.usbserial-540 115200 cs8 -cstopb -parenb -ixon -ixoff -crtscts -echo raw
printf 'TIMESET %s %s\r\n' "$(date -u +%s)" "-300" > /dev/cu.usbserial-540
```

## Soak Script

Reset-cycle soak validation:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/soak_boot.sh 10
```

Manual physical cold-boot matrix helper:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/cold_boot_matrix.sh 20
```

Long refresh soak validation:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/soak_refresh.sh 7200
```

Optional soak env vars:

- `SOAK_WINDOW_SEC` (capture window per cycle, default `8`)
- `SOAK_LOG_DIR` (preserve logs in a fixed path)
- `SOAK_MONITOR_BEFORE` / `SOAK_MONITOR_AFTER`
- `SOAK_REQUIRE_UPTIME=1` (also require first `display uptime screen: ok` marker per cycle)
- `COLD_BOOT_WINDOW_SEC` (cold-boot marker capture window, default `45`)
- `COLD_BOOT_CONNECT_TIMEOUT_SEC` (time to first serial bytes after arm, default `40`)
- `COLD_BOOT_ARM_TIMEOUT_SEC` (time for serial port to reappear after reconnect, default `20`)
- `COLD_BOOT_LOG_DIR` (preserve cold-boot cycle logs)

## Wokwi

`wokwi.toml` points to the `xtensa-esp32-none-elf` debug binary.
