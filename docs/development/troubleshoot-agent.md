# Firmware Troubleshoot Workflow (Agent-First)

This runbook defines how an automation/LLM agent should run and interpret:

```bash
scripts/tests/hw/test_troubleshoot_hw.sh
```

The script is a wrapper around:

```bash
hostctl test troubleshoot
```

with orchestration in:

```text
tools/hostctl/scenarios/troubleshoot.sw.yaml
```

## Goal

- Produce a deterministic troubleshooting pass for firmware issues over UART.
- Prioritize root-cause classification over ad-hoc retries.
- Keep evidence (UART log + soak logs) for follow-up fixes.

## What It Runs

1. Flash firmware using `scripts/device/flash.sh` (unless explicitly disabled).
2. Run UART protocol probes (`PING`, `STATE GET`, `TIMESET`, `PSRAM`).
3. Run boot soak (`scripts/device/soak_boot.sh`) for repeated cold boot markers.
4. Emit summary with pass/fail stage and failure class.

## Preconditions

- Exact board/variant context is known (`esp32` target for this project).
- `HOSTCTL_PORT` points to the intended device in multi-device setups.
- No other process owns the serial port (`lsof <port>` is clean).
- Use `HOSTCTL_*` env vars; the wrapper rejects legacy `ESPFLASH_*` names for hostctl paths.

## Standard Invocation

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/tests/hw/test_troubleshoot_hw.sh
```

Optional arguments:

```bash
scripts/tests/hw/test_troubleshoot_hw.sh [debug|release] [output_log_path]
```

Example:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 \
  scripts/tests/hw/test_troubleshoot_hw.sh debug logs/troubleshoot_manual.log
```

## Agent Control Knobs

- `HOSTCTL_TROUBLESHOOT_FLASH_FIRST` (`1` default)
- `HOSTCTL_TROUBLESHOOT_FLASH_RETRIES` (`2` default)
- `HOSTCTL_TROUBLESHOOT_PROBE_RETRIES` (`6` default)
- `HOSTCTL_TROUBLESHOOT_PROBE_DELAY_MS` (`700` default)
- `HOSTCTL_TROUBLESHOOT_PROBE_TIMEOUT_MS` (`4000` default)
- `HOSTCTL_TROUBLESHOOT_SOAK_CYCLES` (`4` default)

## Deterministic Agent Procedure

1. Set `HOSTCTL_PORT` explicitly.
2. Run `scripts/tests/hw/test_troubleshoot_hw.sh` once with defaults.
3. If it fails, read summary fields: `failure_stage`, `failure_class`, `failure_detail`.
4. Attach artifacts in report:
   - `uart_log=...`
   - `soak_logs=...`
5. Apply one targeted fix for the reported class, then rerun.
6. Do not claim completion unless all of `flash_ok`, `probe_ok`, and `soak_ok` are true.

## Failure-Class Triage Map

- `build`: Build/link/toolchain failure before valid flash.
- `flash`: `flash.sh` failure or flash transport instability.
- `uart_transport`: serial port open/ownership/connectivity issue.
- `uart_protocol`: command/ack contract issue (`PING/STATE/TIMESET/PSRAM`).
- `dhcp_no_ipv4_stall`: Wi-Fi association succeeds but DHCP lease does not converge (no IPv4).
- `runtime`: panic/reset/Guru-style runtime failure signatures.
- `boot`: soak marker gaps across reset cycles.
- `unknown`: insufficient evidence; inspect raw UART log first.

## Required Reporting Fields (Agent Output)

- Command used (including env overrides).
- Final status (`passed` or `failed`).
- `failure_stage` and `failure_class` (if failed).
- Key evidence lines (short excerpt) and full log paths.
- Next single diagnostic step or code fix target.

## Notes

- This workflow intentionally uses `scripts/device/flash.sh` as the flash primitive per project policy.
- Keep retries bounded; repeated failures without new evidence should escalate with collected logs.
