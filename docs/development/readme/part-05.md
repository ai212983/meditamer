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
