# meditamer

## Runtime Stack

The firmware now targets `esp-hal` (`xtensa-esp32-none-elf`) as the primary and only runtime path.

## Documentation Reference

- `statig-event-engine-plan.md`: phased migration plan for replacing in-task heuristic tap logic with a `statig`-based generic sensor-event engine.
- `event-engine-guide.md`: practical developer guide for tuning and modifying the current event engine implementation.
- `sensors.md`: Sensor details and behavior.
- `sound.md`: Sound functionality and behavior.
- `hardware-test-matrix.md`: Hardware testing matrices.
- `reliability-issues.md`: Current ranked reliability risks, evidence, and mitigation gates.
- `troubleshoot-agent.md`: Agent-first runbook for the Serverless Workflow troubleshooting script.

## Git Hooks

This repo uses [`lefthook`](https://github.com/evilmartians/lefthook) to manage hooks (Husky-like, but language-agnostic).

Install dependencies:

```bash
brew install lefthook lychee jq
```

Or via Cargo:

```bash
cargo install --locked lefthook
cargo install --locked lychee
cargo install --locked rust-code-analysis-cli --version 0.0.25
```

Install conventional commit linter:

```bash
go install github.com/conventionalcommit/commitlint@latest
```

Install hooks:

```bash
scripts/ci/setup_hooks.sh
```

Current pre-commit hook:

- Runs `cargo fmt --all` when staged Rust files match `src/**/*.rs`, `tools/**/*.rs`, or `build.rs`.
- Auto-stages formatter edits (`stage_fixed: true`) so commits include rustfmt output.
- Validates links in staged Markdown files via `scripts/ci/check_markdown_links.sh`.
- Uses `lychee` in `--offline` mode by default for reliable local commits.
- Runs host-tooling clippy via `scripts/ci/lint_host_tools.sh` (`-D warnings`) when staged files touch `tools/**` or workspace toolchain manifests.

Current commit-msg hook:

- Validates commit messages against Conventional Commits via `scripts/ci/check_commit_message.sh` and `commitlint`.
- Requires a scope in `type(scope): subject` format.
- Enforces allowed scopes: `runtime`, `touch`, `event-engine`, `storage`, `upload`, `wifi`, `telemetry`, `graphics`, `drivers`, `tooling`, `ci`, `docs`.
- Exempts Git-generated/autosquash subjects (`Merge ...`, `Revert ...`, `fixup! ...`, `squash! ...`) from custom scope checks.

Current pre-push hook:

- Runs strict firmware clippy via `cargo clippy --locked --all-features --workspace --bins --lib -- -D warnings` when pushed files touch firmware/workspace Rust paths.
- Runs strict code-metrics ratchet via `RCA_ENFORCE=1 RCA_RATCHET=1 scripts/ci/lint_code_analysis.sh` on Rust/workspace changes.

Code analysis lint command (report mode by default):

```bash
scripts/ci/lint_code_analysis.sh
```

Strict ratchet mode (used by pre-push and CI):

```bash
RCA_ENFORCE=1 RCA_RATCHET=1 scripts/ci/lint_code_analysis.sh
```

Refresh ratchet baseline after intentional refactors:

```bash
RCA_UPDATE_BASELINE=1 scripts/ci/lint_code_analysis.sh
```

Rust-analyzer baseline lint:

```bash
scripts/ci/lint_rust_analyzer.sh
```

Notes for this workspace:

- The firmware is `no_std` with heavy feature/cfg gating; analyzer results can include inactive-code and unresolved-import noise outside active build paths.
- The baseline script intentionally runs with `--disable-build-scripts --disable-proc-macros` for stable, fast CI signal.
- Authoritative correctness gates remain `cargo check` and strict `cargo clippy` on `--bins --lib`.

Formatting enforcement:

- CI enforces Rust formatting via `cargo +stable fmt --all -- --check` in `.github/workflows/rust_ci.yml` (`PR Light CI` -> `Rust Format` job).

Optional full (online) validation:

```bash
git ls-files -z '*.md' | xargs -0 env MARKDOWN_LINKS_ONLINE=1 scripts/ci/check_markdown_links.sh
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
scripts/build/build.sh [debug|release]
```

Default is `release` when no argument is provided.

The default Xtensa runner (`scripts/build/xtensa_runner.sh`) flashes firmware without opening
an interactive monitor (safe in non-interactive shells). To enable monitor explicitly:

```bash
ESPFLASH_RUN_MONITOR=1 cargo run
```

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

## Runtime Metrics

Runtime metrics are available over `UART0` (`115200` baud):

```text
METRICS
```

Response lines:

```text
METRICS MARBLE_REDRAW_MS=<n> MAX_MS=<n>
METRICS WIFI attempt=<n> success=<n> failure=<n> no_ap=<n> scan_runs=<n> scan_empty=<n> scan_hits=<n>
METRICS UPLOAD accept_ok=<n> accept_err=<n> request_err=<n> req_hdr_to=<n> req_read_body=<n> req_sd_busy=<n> sd_errors=<n> sd_busy=<n> sd_timeouts=<n> sd_power_on_fail=<n> sd_init_fail=<n> sess_timeout_abort=<n> sess_mode_off_abort=<n>
METRICS UPLOAD_PHASE req=<n> bytes=<n> body_ms=<n> body_max=<n> sd_ms=<n> sd_max=<n> req_ms=<n> req_max=<n>
METRICS UPLOAD_RTT begin_n=<n> begin_ms=<n> begin_max=<n> chunk_n=<n> chunk_ms=<n> chunk_max=<n> commit_n=<n> commit_ms=<n> commit_max=<n> abort_n=<n> abort_ms=<n> abort_max=<n> mkdir_n=<n> mkdir_ms=<n> mkdir_max=<n> rm_n=<n> rm_ms=<n> rm_max=<n>
METRICS NET wifi_connected=<0|1> http_listening=<0|1> ip=<a.b.c.d>
```

`UPLOAD_PHASE` reports end-to-end per-request timing buckets for upload body handling.
`UPLOAD_RTT` reports SD roundtrip counts and timing totals/maxima by command phase.

### Runtime Telemetry Domain Control

Use runtime telemetry domain toggles to reduce log pressure without reflashing.

```text
TELEM
TELEMSET NONE
TELEMSET WIFI ON
TELEMSET NET ON
TELEMSET REASSOC ON
```

- `TELEM` returns current domain mask/status.
- `TELEMSET` updates enabled domains (`WIFI`, `REASSOC`, `NET`, `HTTP`, `SD`, `ALL`, `DEFAULT`, `NONE`).
- `METRICS` / `METRICSNET` remain available regardless of telemetry domain settings.

Agent-oriented contract and runbook:

- `docs/development/telemetry-control-agent.md`

## SD Card Hardware Test

Automated UART-driven SD/FAT end-to-end validation:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/tests/hw/test_sdcard_hw.sh
```

Defaults:

- uses current flashed firmware (does **not** flash by default)
- captures monitor log under `logs/`
- default suite (`HOSTCTL_SDCARD_SUITE=all`) verifies:
  - baseline flow: `SDPROBE`, FAT mkdir/write/read/append/stat/truncate/rename/remove, and `SDRWVERIFY`
  - burst/backpressure flow: burst command sequence without host pacing
  - failure-path flow: non-empty-dir remove rejection, rename collision rejection, not-found read, `SDRWVERIFY 0` refusal, parser `CMD ERR` for oversized payload
  - command completion via `SDREQ id=...` + `SDWAIT <id>` with status/code checks

Optional env vars:

- `HOSTCTL_SDCARD_FLASH_FIRST=1` to flash first (mode arg defaults to `debug`)
- `HOSTCTL_SDCARD_VERIFY_LBA` (default `2048`)
- `HOSTCTL_SDCARD_BASE_PATH` to override test directory path on SD card
- `HOSTCTL_SDCARD_SUITE` (`all` default, `baseline`, `burst`, or `failures`)
- `HOSTCTL_SDCARD_SDWAIT_TIMEOUT_MS` (default `300000`)

Burst/backpressure regression only:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/tests/hw/test_sdcard_burst_regression.sh
```

## SD Asset Upload Over Wi-Fi (STA, HTTP)

Upload server for pushing assets to SD card without removing it.

Build/flash:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/device/flash.sh debug
```

Notes:

- optional compile-time credentials are still supported via `MEDITAMER_WIFI_SSID` / `MEDITAMER_WIFI_PASSWORD`
  (fallback `SSID` / `PASSWORD`).
- upload service must be enabled at runtime (`STATE SET upload=on`).
- hard-cut runtime network control now uses `NET*` UART commands only.
- server listens on port `8080` after DHCP lease.
- when an upload token is configured, all HTTP endpoints except `/health` require an `x-upload-token` header;
  requests without a valid token are rejected.
- if neither `MEDITAMER_UPLOAD_HTTP_TOKEN` nor `UPLOAD_HTTP_TOKEN` is set at build time, authentication is
  disabled and non-`/health` endpoints accept requests without an `x-upload-token` header.
- configure the token at build time with `MEDITAMER_UPLOAD_HTTP_TOKEN` (fallback: `UPLOAD_HTTP_TOKEN`).
- mutating endpoints (`/mkdir`, `/rm`, `/upload*`) are limited to the `/assets` subtree.

Runtime network policy/config provisioning over UART:

```text
NETCFG SET {"ssid":"<ssid>","password":"<password>","connect_timeout_ms":30000,"dhcp_timeout_ms":20000,"pinned_dhcp_timeout_ms":45000,"listener_timeout_ms":25000,"scan_active_min_ms":600,"scan_active_max_ms":1500,"scan_passive_ms":1500,"retry_same_max":2,"rotate_candidate_max":2,"rotate_auth_max":5,"full_scan_reset_max":1,"driver_restart_max":1,"cooldown_ms":1200,"driver_restart_backoff_ms":2500}
```

Read current runtime config:

```text
NETCFG GET
```

Start/stop/recover/status:

```text
NET START
NET STOP
NET RECOVER
NET STATUS
NET LISTENER ON
NET LISTENER OFF
```

Credential persistence:

- `NETCFG SET` with `ssid` persists credentials to SD file `/config/wifi.cfg`.
- On boot, firmware attempts to load `/config/wifi.cfg` before waiting for runtime `NETCFG SET`.
- This survives reboot and firmware reflashes (as long as SD card content is retained).

Wi-Fi acceptance helper (hard-cut):

```bash
HOSTCTL_NET_PORT=/dev/cu.usbserial-510 \
HOSTCTL_NET_BAUD=115200 \
HOSTCTL_NET_SSID='<wifi-ssid>' \
HOSTCTL_NET_PASSWORD='<wifi-password>' \
HOSTCTL_NET_POLICY_PATH=./tools/hostctl/scenarios/wifi-policy.default.json \
HOSTCTL_NET_LOG_PATH=./logs/wifi_acceptance_manual.log \
scripts/tests/hw/test_wifi_acceptance.sh
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
scripts/assets/upload_assets_http.sh --host <device-ip> --src assets --dst /assets
```

Upload a single file:

```bash
scripts/assets/upload_assets_http.sh --host <device-ip> --src ./path/to/file.bin --dst /assets
```

Optional upload helper tuning:

- `HOSTCTL_UPLOAD_CHUNK_SIZE` controls chunk size in bytes for `/upload_chunk` fallback flow (default `8192`).

Delete paths (relative to `--dst`, or absolute under `/assets`):

```bash
scripts/assets/upload_assets_http.sh --host <device-ip> --dst /assets --rm old.bin --rm unused/
```

Suggested runtime flow:

1. `STATE SET upload=on`
2. `NETCFG SET {...}`
3. `NET START`
4. poll `NET STATUS` until `state="Ready"` and non-zero IPv4
3. Upload files over HTTP
4. `STATE SET upload=off`

Wi-Fi acceptance workflow:

```bash
scripts/tests/hw/test_wifi_acceptance.sh
```

- runs via `hostctl test wifi-acceptance` behind the script wrapper.
- strategy execution is declarative (`tools/hostctl/scenarios/wifi-acceptance.sw.yaml`) with primitive hostctl actions.
- consumes only `HOSTCTL_NET_*` environment contract.
- readiness uses structured firmware frames (`NET_STATUS {...}`), not monitor-tail text parsing.

Wi-Fi zero-discovery diagnostic workflow:

```bash
HOSTCTL_NET_PORT=/dev/cu.usbserial-540 \
HOSTCTL_NET_BAUD=115200 \
HOSTCTL_NET_SSID='<wifi-ssid>' \
HOSTCTL_NET_PASSWORD='***' \
HOSTCTL_NET_POLICY_PATH=./tools/hostctl/scenarios/wifi-policy.default.json \
HOSTCTL_NET_DISCOVERY_PROFILE_PATH=./tools/hostctl/scenarios/wifi-discovery-debug.default.toml \
HOSTCTL_NET_LOG_PATH=./logs/wifi_discovery_debug_manual.log \
scripts/tests/hw/test_wifi_discovery_debug.sh
```

- runs via `hostctl test wifi-discovery-debug` behind the script wrapper.
- strategy and pass/fail thresholds are declarative TOML in
  `tools/hostctl/scenarios/wifi-discovery-debug.default.toml`.
- default discovery profile temporarily disables HTTP listener during probe rounds
  (`disable_listener_during_probe_rounds=true`) to reduce radio/memory pressure
  while preserving Wi-Fi discovery.
- workflow orchestration remains declarative in
  `tools/hostctl/scenarios/wifi-discovery-debug.sw.yaml`.
- reports round-level counters for:
  - zero-result scan events
  - non-zero scan events
  - `no_ap_found` disconnect events
  - target SSID visibility.

## Hostctl Workflow Authoring

- Authoring guide for declarative host workflows:
  `docs/development/hostctl-workflow-authoring.md`
- Scenario files live in `tools/hostctl/scenarios/*.sw.yaml`.
- Keep retry/branch strategy in YAML; keep Rust runtime actions primitive.

## Firmware Troubleshoot Workflow (Serverless Workflow DSL)

Run a UART-centric troubleshooting sequence (flash, protocol probes, boot soak):

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/tests/hw/test_troubleshoot_hw.sh
```

- runs through `hostctl test troubleshoot` with declarative orchestration in
  `tools/hostctl/scenarios/troubleshoot.sw.yaml`
- uses `scripts/device/flash.sh` as the flash primitive (per project flash policy)
- classifies failures into `build`, `flash`, `boot`, `runtime`,
  `uart_protocol`, `uart_transport`, or `unknown`
- emits summary plus persistent UART and soak logs under `logs/`

Optional env vars:

- `HOSTCTL_TROUBLESHOOT_FLASH_FIRST` (`1` default)
- `HOSTCTL_TROUBLESHOOT_FLASH_RETRIES` (`2` default)
- `HOSTCTL_TROUBLESHOOT_PROBE_RETRIES` (`6` default)
- `HOSTCTL_TROUBLESHOOT_PROBE_DELAY_MS` (`700` default)
- `HOSTCTL_TROUBLESHOOT_PROBE_TIMEOUT_MS` (`4000` default)
- `HOSTCTL_TROUBLESHOOT_SOAK_CYCLES` (`4` default)

Agent-oriented contract and runbook:

- `docs/development/troubleshoot-agent.md`

## Soak Script

Reset-cycle soak validation:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/device/soak_boot.sh 10
```

Manual physical cold-boot matrix helper:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/device/cold_boot_matrix.sh 20
```

Long refresh soak validation:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/device/soak_refresh.sh 7200
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
