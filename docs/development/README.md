# meditamer

## Runtime Stack

The firmware now targets `esp-hal` (`xtensa-esp32-none-elf`) as the primary and only runtime path.

## Documentation Reference

- `statig-event-engine-plan.md`: phased migration plan for replacing in-task heuristic tap logic with a `statig`-based generic sensor-event engine.
- `event-engine-guide.md`: practical developer guide for tuning and modifying the current event engine implementation.
- `sensors.md`: Sensor details and behavior.
- `sound.md`: Sound functionality and behavior.
- `hardware-test-matrix.md`: Hardware testing matrices.

## Git Hooks

This repo uses [`lefthook`](https://github.com/evilmartians/lefthook) to manage hooks (Husky-like, but language-agnostic).

Install dependencies:

```bash
brew install lefthook lychee
```

Or via Cargo:

```bash
cargo install --locked lefthook
cargo install --locked lychee
```

Install conventional commit linter:

```bash
go install github.com/conventionalcommit/commitlint@latest
```

Install hooks:

```bash
scripts/setup_hooks.sh
```

Current pre-commit hook:

- Validates links in staged Markdown files via `scripts/check_markdown_links.sh`.
- Uses `lychee` in `--offline` mode by default for reliable local commits.
- Runs host-tooling clippy via `scripts/lint_host_tools.sh` (`-D warnings`) when staged files touch `tools/**` or workspace toolchain manifests.

Current commit-msg hook:

- Validates commit messages against Conventional Commits via `scripts/check_commit_message.sh` and `commitlint`.

Current pre-push hook:

- Runs strict firmware clippy via `cargo clippy --locked --all-features --workspace --bins --lib -- -D warnings` when pushed files touch firmware/workspace Rust paths.

Optional full (online) validation:

```bash
git ls-files -z '*.md' | xargs -0 env MARKDOWN_LINKS_ONLINE=1 scripts/check_markdown_links.sh
```

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

The default Xtensa runner (`scripts/xtensa_runner.sh`) flashes firmware without opening
an interactive monitor (safe in non-interactive shells). To enable monitor explicitly:

```bash
ESPFLASH_RUN_MONITOR=1 cargo run
```

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

## Allocator Diagnostics

Firmware accepts allocator status commands on `UART0` (`115200` baud):

```text
PSRAM
```

Aliases: `HEAP`, `ALLOCATOR`.

Response format:

```text
PSRAM feature_enabled=<bool> state=<state> total_bytes=<n> used_bytes=<n> free_bytes=<n> peak_used_bytes=<n>
```

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

## SD Card Hardware Test

Automated UART-driven SD/FAT end-to-end validation:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/test_sdcard_hw.sh
```

Defaults:

- uses current flashed firmware (does **not** flash by default)
- captures monitor log under `logs/`
- default suite (`SDCARD_TEST_SUITE=all`) verifies:
  - baseline flow: `SDPROBE`, FAT mkdir/write/read/append/stat/truncate/rename/remove, and `SDRWVERIFY`
  - burst/backpressure flow: burst command sequence without host pacing
  - failure-path flow: non-empty-dir remove rejection, rename collision rejection, not-found read, `SDRWVERIFY 0` refusal, parser `CMD ERR` for oversized payload
  - command completion via `SDREQ id=...` + `SDWAIT <id>` with status/code checks

Optional env vars:

- `SDCARD_TEST_FLASH_FIRST=1` to flash first (mode arg defaults to `debug`)
- `SDCARD_TEST_VERIFY_LBA` (default `2048`)
- `SDCARD_TEST_BASE_PATH` to override test directory path on SD card
- `SDCARD_TEST_SUITE` (`all` default, `baseline`, `burst`, or `failures`)
- `SDCARD_TEST_SDWAIT_TIMEOUT_MS` (default `300000`)

Burst/backpressure regression only:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/test_sdcard_burst_regression.sh
```

## SD Asset Upload Over Wi-Fi (STA, HTTP)

Feature-gated upload server (disabled by default) for pushing assets to SD card without removing it.

Build/flash with feature:

```bash
export CARGO_FEATURES=asset-upload-http
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/flash.sh debug
```

Notes:

- optional compile-time credentials are still supported via `MEDITAMER_WIFI_SSID` / `MEDITAMER_WIFI_PASSWORD`
  (fallback `SSID` / `PASSWORD`).
- if credentials are not compiled in, firmware waits for UART `WIFISET` command.
- server listens on port `8080` after DHCP lease (`upload_http: listening on <ip>:8080` in logs).

Runtime credential provisioning over UART:

```text
WIFISET <ssid> <password>
```

Open network (no password):

```text
WIFISET <ssid>
```

Credential persistence:

- `WIFISET` now persists credentials to SD file `/config/wifi.cfg`.
- On boot, firmware attempts to load `/config/wifi.cfg` before waiting for UART `WIFISET`.
- This survives reboot and firmware reflashes (as long as SD card content is retained).

Host helper:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-510 scripts/wifiset.sh <ssid> <password>
```

Health check:

```bash
curl "http://<device-ip>:8080/health"
```

Create directory:

```bash
curl -X POST "http://<device-ip>:8080/mkdir?path=/assets/images"
```

Delete file or empty directory:

```bash
curl -X DELETE "http://<device-ip>:8080/rm?path=/assets/old.bin"
```

Upload an assets directory:

```bash
scripts/upload_assets_http.py --host <device-ip> --src assets --dst /assets
```

Upload a single file:

```bash
scripts/upload_assets_http.py --host <device-ip> --src ./path/to/file.bin --dst /assets
```

Delete paths (absolute or relative to `--dst`):

```bash
scripts/upload_assets_http.py --host <device-ip> --dst /assets --rm old.bin --rm unused/
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
