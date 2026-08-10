## Firmware Troubleshoot Workflow (Serverless Workflow DSL)

Run a UART-centric troubleshooting sequence (flash, protocol probes, boot soak):

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/tests/hw/test_troubleshoot_hw.sh
```

- runs through `hostctl test troubleshoot` with declarative orchestration in
  `tools/hostctl/scenarios/troubleshoot.sw.yaml`
- uses `scripts/device/flash.sh` as the flash primitive (per project flash policy)
  and that wrapper runs `hostctl flash-capture` with orchestration in
  `tools/hostctl/scenarios/flash-capture.sw.yaml`
- classifies failures into `build`, `flash`, `boot`, `runtime`,
  `uart_protocol`, `uart_transport`, or `unknown`
- emits summary plus persistent UART and soak logs under `logs/`
- boot-phase flash evidence should come from flash-capture artifacts; use
  `scripts/device/monitor.sh` only for passive follow-up attach

Optional env vars:

- `HOSTCTL_TROUBLESHOOT_FLASH_FIRST` (`1` default)
- `HOSTCTL_TROUBLESHOOT_FLASH_RETRIES` (`2` default)
- `HOSTCTL_TROUBLESHOOT_PROBE_RETRIES` (`6` default)
- `HOSTCTL_TROUBLESHOOT_PROBE_DELAY_MS` (`700` default)
- `HOSTCTL_TROUBLESHOOT_PROBE_TIMEOUT_MS` (`4000` default)
- `HOSTCTL_TROUBLESHOOT_SOAK_CYCLES` (`4` default)

Agent-oriented contract and runbook:

- `docs/development/troubleshoot-agent.md`
- `docs/development/hostctl-workflow-authoring.md`

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

## UI Lifecycle Evidence

After flashing the exact debug or release artifact under test, run at least two
complete Home → Launcher → diagnostics → Home cycles:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 \
cargo run --locked --manifest-path tools/hostctl/Cargo.toml -- \
  test ui-lifecycle --cycles 2 --output logs/ui-lifecycle.log
```

The default settled-baseline tolerance is exactly zero. After an identified
artifact has a recorded characterization run, an acceptance run may opt into
the predeclared bound from that evidence. E-0006 characterizes the current
128 KiB LVGL arena at a 256-byte allocator-settling band:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 \
cargo run --locked --manifest-path tools/hostctl/Cargo.toml -- \
  test ui-lifecycle --cycles 50 --max-baseline-drift-bytes 256 \
  --output logs/ui-lifecycle-acceptance.log
```

The `UISTEP` primitive performs one shell-owned transition and acknowledges it
only after panel refresh. The workflow never retries a timeout because its
outcome is ambiguous. It writes the raw UART log and a sibling JSON report,
then fails closed on lifecycle count/route gaps, out-of-bound settled LVGL
usage/usable-total drift, changed live-block counts, a non-plateaued high-water,
heap drift, shell or allocator-integrity faults, refresh errors, panics, or
watchdogs.
Passing logs remain resource evidence only: observe the physical panel and
touch path before closing the UI phase gate.

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

## Rust ESP-IDF Wi-Fi Control Probe

Standalone Rust-on-ESP-IDF scan probe:

```bash
IDF_APP_ROOT="$HOME/.esp-idf/v5.3.4" \
IDF_TOOLS_PATH="$HOME/.espressif" \
scripts/device/wifi_control_idf_rust.sh build
```

Flash and monitor:

```bash
IDF_APP_ROOT="$HOME/.esp-idf/v5.3.4" \
IDF_TOOLS_PATH="$HOME/.espressif" \
ESPFLASH_PORT=/dev/cu.usbserial-540 \
scripts/device/wifi_control_idf_rust.sh flash

IDF_APP_ROOT="$HOME/.esp-idf/v5.3.4" \
IDF_TOOLS_PATH="$HOME/.espressif" \
ESPFLASH_PORT=/dev/cu.usbserial-540 \
scripts/device/wifi_control_idf_rust.sh monitor
```

Notes:

- current crate set (`esp-idf-svc 0.51`, `esp-idf-hal 0.45.2`) requires a
  supported ESP-IDF 5.3.x install for this probe
- ESP-IDF 5.5.2 currently fails this Rust probe build in `esp-idf-hal` on TWAI
  bindings, so use `v5.3.4` for the comparison path
