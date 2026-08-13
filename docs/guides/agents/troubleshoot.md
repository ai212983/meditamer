# Firmware Troubleshoot Workflow (Agent-First)

This runbook defines how an automation/LLM agent should run and interpret:

```bash
scripts/hostctl.sh test troubleshoot
```

`scripts/hostctl.sh` is the direct native-launch wrapper for `hostctl` (Cargo
target/toolchain/env preparation only; it does not add or resolve any
troubleshoot-specific arguments). The `troubleshoot` subcommand itself is
orchestrated in:

```text
tools/hostctl/scenarios/troubleshoot.sw.yaml
```

## Goal

- Produce a deterministic troubleshooting pass for firmware issues over UART.
- Prioritize root-cause classification over ad-hoc retries.
- Keep evidence (UART log + soak logs) for follow-up fixes.

## What It Runs

1. Flash firmware using `scripts/device/flash.sh` (unless explicitly disabled).
   This wrapper runs `hostctl flash-capture`, which is orchestrated by
   `tools/hostctl/scenarios/flash-capture.sw.yaml`.
2. Run UART protocol and readiness probes (`PING`, `STATE GET`, `PSRAM`).
3. Run boot soak (`scripts/device/soak_boot.sh`) for repeated cold boot markers.
4. Emit summary with pass/fail stage and failure class.

## Preconditions

- Exact board/variant context is known (`esp32` target for this project).
- `HOSTCTL_PORT` points to the intended device in multi-device setups.
- Prefer `/dev/cu.*` ports over `/dev/tty.*` on macOS.
- No other process owns the serial port (`lsof <port>` is clean).
- Use `HOSTCTL_*` env vars for the underlying workflow's control knobs (below);
  `scripts/hostctl.sh` itself only understands `HOSTCTL_PORT`/`HOSTCTL_PORT_HINT`-style
  launch env, not command-specific ones.

## Standard Invocation

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/hostctl.sh test troubleshoot --build-mode debug
```

Optional arguments (`hostctl test troubleshoot --help`):

```bash
scripts/hostctl.sh test troubleshoot --build-mode [debug|release] [--output <output_log_path>]
```

`--output`, like every path argument on this direct launcher, is not resolved
relative to your shell's working directory -- pass an absolute path.

Example:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 \
  scripts/hostctl.sh test troubleshoot --build-mode debug --output "$(pwd)/logs/troubleshoot_manual.log"
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
2. Run `scripts/hostctl.sh test troubleshoot --build-mode debug` once with defaults.
3. If it fails, read summary fields: `failure_stage`, `failure_class`, `failure_detail`.
   - for runtime failures, `failure_detail` may include `runtime_subclass=...`
     (`runtime_panic_guru`, `runtime_panic_stack`, `runtime_panic_assert`, `runtime_panic_other`, `runtime_unexpected_reboot`)
4. Attach artifacts in report:
   - `uart_log=...`
   - `soak_logs=...`
   - `flash.log`, `capture.log`, `summary.txt` from the flash-capture artifact
     directory when flash ran
5. Apply one targeted fix for the reported class, then rerun.
6. Do not claim completion unless all of `flash_ok`, `probe_ok`, and `soak_ok` are true.

## Failure-Class Triage Map

- `build`: Build/link/toolchain failure before valid flash.
- `flash`: `flash.sh` failure or flash transport instability.
- `uart_transport`: serial port open/ownership/connectivity issue.
- `uart_protocol`: command/response or readiness issue (`PING/STATE/PSRAM`).
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
- For early boot evidence, rely on flash-capture artifacts, not `espflash monitor` reset flows.
- Keep retries bounded; repeated failures without new evidence should escalate with collected logs.
