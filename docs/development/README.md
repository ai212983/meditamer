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

- Runs `cargo fmt --all` when staged Rust files match `src/**/*.rs`, `tools/**/*.rs`, or `build.rs`.
- Auto-stages formatter edits (`stage_fixed: true`) so commits include rustfmt output.
- Validates links in staged Markdown files via `scripts/check_markdown_links.sh`.
- Uses `lychee` in `--offline` mode by default for reliable local commits.
- Runs host-tooling clippy via `scripts/lint_host_tools.sh` (`-D warnings`) when staged files touch `tools/**` or workspace toolchain manifests.

Current commit-msg hook:

- Validates commit messages against Conventional Commits via `scripts/check_commit_message.sh` and `commitlint`.

Current pre-push hook:

- Runs strict firmware clippy via `cargo clippy --locked --all-features --workspace --bins --lib -- -D warnings` when pushed files touch firmware/workspace Rust paths.

Formatting enforcement:

- CI enforces Rust formatting via `cargo +stable fmt --all -- --check` in `.github/workflows/rust_ci.yml` (`PR Light CI` -> `Rust Format` job).

Optional full (online) validation:

```bash
git ls-files -z '*.md' | xargs -0 env MARKDOWN_LINKS_ONLINE=1 scripts/check_markdown_links.sh
```

## File Size Guidelines (Rewrite Phase)

These limits are active during the current rewrite on this branch. Enforcement is manual in review for now (no hooks yet).

- Hard cap: non-generated source files must stay at or below `500` lines.
- Split-plan trigger: once a file crosses `420` lines, the same PR must include a short split plan.
- Warning threshold: treat `450` lines as "split now unless there is a blocking reason".
- New modules target: keep new modules at or below `300` lines.
- Prefer folder-based splits over flat suffix files. Example: prefer `src/firmware/event_engine/tap/hsm.rs` and `src/firmware/event_engine/tap/trace.rs` over `src/firmware/event_engine/tap_hsm.rs` and `src/firmware/event_engine/tap_trace.rs`.
- Generated/build outputs are excluded from these limits (for example `target/**` and `**/out/**`).

Suggested PR checklist line:

- `[]` If any touched file is `>= 420` lines, I included a split plan in this PR description.

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

Recommended invocation (stable + deterministic):

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 FLASH_SET_TIME_AFTER_FLASH=0 scripts/flash.sh debug
```

Optional flash env vars:

- `ESPFLASH_BAUD` (default `460800` in `scripts/flash.sh`)
- `FLASH_TIMEOUT_SEC` (default `360`; watchdog timeout per primary flash attempt)
- `FLASH_STATUS_INTERVAL_SEC` (default `15`; heartbeat interval while flashing)
- `ESPFLASH_ENABLE_FALLBACK` (`1` default; retries with `--no-stub` on failure/timeout)
- `ESPFLASH_FALLBACK_BAUD` (default `115200`)
- `ESPFLASH_SKIP_UPDATE_CHECK` (`1` default; avoids crates.io version-check delay)
- `FLASH_SET_TIME_AFTER_FLASH` (`1` default; set `0` to skip automatic `TIMESET`)

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

### Flash Troubleshooting

If flashing appears "stuck":

- `scripts/flash.sh` now prints `Flashing in progress...` every `FLASH_STATUS_INTERVAL_SEC` seconds.
- A flash watchdog aborts after `FLASH_TIMEOUT_SEC`; with fallback enabled, it retries automatically using `--no-stub`.

If serial port is busy:

```bash
lsof /dev/cu.usbserial-540
```

Stop monitor/holder processes, then re-run flash.

Force slow fallback path directly:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 ESPFLASH_BAUD=115200 ESPFLASH_ENABLE_FALLBACK=0 scripts/flash.sh debug
```

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

### Defmt Telemetry

Firmware supports optional `defmt` telemetry via feature `telemetry-defmt`.

Build/flash with defmt telemetry enabled:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 CARGO_FEATURES=telemetry-defmt scripts/flash.sh debug
```

Use espflash monitor mode (not raw cat/tio) to decode defmt frames:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 ESPFLASH_MONITOR_MODE=espflash scripts/monitor.sh
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

## Runtime Service Modes

Runtime mode controls are available over `UART0` (`115200` baud):

```text
MODE STATUS
MODE UPLOAD ON
MODE UPLOAD OFF
MODE ASSETS ON
MODE ASSETS OFF
```

Compatibility alias:

```text
RUNMODE UPLOAD
RUNMODE NORMAL
```

Notes:

- `MODE` state is persisted in flash and restored on boot.
- `MODE` / `RUNMODE` return `OK` only after the mode update is applied by runtime tasks.
- `MODE UPLOAD OFF` rejects upload operations and releases upload transfer buffers.
- `MODE ASSETS OFF` disables SD asset reads, clears runtime graphics cache, and releases asset-read transfer buffers.
- On `psram-alloc` builds, transfer buffers are allocated in PSRAM on-demand and released when the mode is disabled.

Quick RAM check sequence:

```text
PSRAM
MODE UPLOAD ON
PSRAM
MODE UPLOAD OFF
PSRAM
MODE ASSETS OFF
PSRAM
MODE ASSETS ON
PSRAM
```

Automated smoke run (mode toggles + PSRAM snapshots):

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/runtime_modes_smoke.sh
```

Optional env var:

- `MODE_SMOKE_SETTLE_MS` (default `0`; can be raised if extra post-command delay is desired)

## Runtime Metrics

Runtime metrics are available over `UART0` (`115200` baud):

```text
METRICS
```

Response lines:

```text
METRICS MARBLE_REDRAW_MS=<n> MAX_MS=<n>
METRICS WIFI attempt=<n> success=<n> failure=<n> no_ap=<n> scan_runs=<n> scan_empty=<n> scan_hits=<n>
METRICS UPLOAD accept_err=<n> request_err=<n> sd_errors=<n> sd_busy=<n> sd_timeouts=<n> sd_power_on_fail=<n> sd_init_fail=<n>
METRICS NET wifi_connected=<0|1> http_listening=<0|1> ip=<a.b.c.d>
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

Upload server for pushing assets to SD card without removing it.

Build/flash:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/flash.sh debug
```

Notes:

- optional compile-time credentials are still supported via `MEDITAMER_WIFI_SSID` / `MEDITAMER_WIFI_PASSWORD`
  (fallback `SSID` / `PASSWORD`).
- upload service must be enabled at runtime (`MODE UPLOAD ON`).
- if credentials are not compiled in, firmware waits for UART `WIFISET` command.
- server listens on port `8080` after DHCP lease; scripts should poll `METRICS NET` and use `ip=<a.b.c.d>` when `http_listening=1` instead of parsing async logs.
- when an upload token is configured, all HTTP endpoints except `/health` require an `x-upload-token` header;
  requests without a valid token are rejected.
- if neither `MEDITAMER_UPLOAD_HTTP_TOKEN` nor `UPLOAD_HTTP_TOKEN` is set at build time, authentication is
  disabled and non-`/health` endpoints accept requests without an `x-upload-token` header.
- configure the token at build time with `MEDITAMER_UPLOAD_HTTP_TOKEN` (fallback: `UPLOAD_HTTP_TOKEN`).
- mutating endpoints (`/mkdir`, `/rm`, `/upload*`) are limited to the `/assets` subtree.

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

Create directory (authenticated endpoint):

```bash
UPLOAD_TOKEN=<your-upload-token>
curl -X POST \
  -H "x-upload-token: ${UPLOAD_TOKEN}" \
  "http://<device-ip>:8080/mkdir?path=/assets/images"
```

Delete file or empty directory (authenticated endpoint):

```bash
curl -X DELETE \
  -H "x-upload-token: ${UPLOAD_TOKEN}" \
  "http://<device-ip>:8080/rm?path=/assets/old.bin"
```

Upload an assets directory:

```bash
scripts/upload_assets_http.py --host <device-ip> --src assets --dst /assets
```

Upload a single file:

```bash
scripts/upload_assets_http.py --host <device-ip> --src ./path/to/file.bin --dst /assets
```

Delete paths (relative to `--dst`, or absolute under `/assets`):

```bash
scripts/upload_assets_http.py --host <device-ip> --dst /assets --rm old.bin --rm unused/
```

Suggested runtime flow:

1. `MODE UPLOAD ON`
2. `WIFISET <ssid> <password>` (if needed)
3. Upload files over HTTP
4. `MODE UPLOAD OFF`

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
