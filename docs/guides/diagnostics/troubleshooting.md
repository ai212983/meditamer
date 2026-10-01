# Troubleshooting

Use this guide to classify a failure. Repeatable qualification suites live in
[device validation](device-validation.md), [network validation](../network/validation.md),
and [observation validation](../observations/validation.md).

## Run and retain evidence

Identify the board/target and port; this flash/boot workflow is for Inkplate ESP32.
Stop other serial owners (`lsof "$DEVICE_PORT"`). Prefer `/dev/cu.*` on macOS.
The troubleshoot workflow always builds/flashes. To preserve an installed
diagnostic image, use the [passive monitor](../development/build-and-flash.md#monitor) and
read-only [serial queries](../../references/runtime/serial-control.md) instead.
For a rebuild-and-boot check:

```bash
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh test troubleshoot \
  --build-mode debug --output logs/<new-troubleshoot-run>.log
```

[The YAML](../../../tools/hostctl/scenarios/troubleshoot.sw.yaml) orchestrates
flash-capture, PING/STATE/PSRAM probes, and reset-cycle boot soak. Relative paths
resolve from the repository root. Use flash-capture artifacts for early boot;
[passive monitor](../development/build-and-flash.md#monitor) is for follow-up attachment.

The recipe fixes two flash retries, six protocol probes, and four reset-cycle
soaks. These are internal workflow constants, not environment overrides.

## Interpret failure

Read `failure_stage`, `failure_class`, and `failure_detail` first:

| Class | Investigate |
| --- | --- |
| `build` | Compilation, link, or toolchain before a valid flash |
| `flash` | Flash primitive or transport |
| `uart_transport` | Serial ownership, open, or connectivity |
| `uart_protocol` | Readiness/command response mismatch |
| `dhcp_no_ipv4_stall` | Association succeeded but no DHCP lease |
| `runtime` | Panic, reset, Guru, stack/assert signatures |
| `boot` | Missing markers across reset cycles |
| `unknown` | Raw UART evidence before assigning a cause |

Runtime details can include panic subclasses or `runtime_unexpected_reboot`.
Report the command/overrides, status, failure fields, short evidence excerpt,
`uart_log`/`soak_logs`, and flash `flash.log`/`capture.log`/`summary.txt` when present.
Choose one next diagnostic step or targeted fix. Do not claim a workflow pass
unless `flash_ok`, `probe_ok`, and `soak_ok` are true.

## Diagnostic interpretation

Correlate the failure with [persistence/RTC breadcrumbs and radio-off snapshots](../../references/runtime/metrics.md#persistence-and-rtc-diagnostics)
before selecting a follow-up test. For RTC `QueueTimeout`, correlate the
`I2C_PROXY_TIMEOUT` and `I2C_PROXY_OWNER` records described there before changing
admission policy. For startup I²C failures, preserve the complete
boot log and compare repeated captures using the [startup diagnostic fields and
inference limits](../../references/runtime/metrics.md#startup-i2c-diagnostics).
