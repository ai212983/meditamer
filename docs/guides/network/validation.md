# Network validation

Run this workflow before landing Wi-Fi, network-stack or upload changes.
[Wi-Fi upload](asset-upload.md) owns provisioning and file operations;
[network reference](../../references/network-and-upload.md) owns protocol/configuration.

## Preconditions and acceptance

Copy `.env.example` to untracked `.env.local`, then set `HOSTCTL_NET_PORT`,
`HOSTCTL_NET_BAUD`, `HOSTCTL_NET_SSID`, and `HOSTCTL_NET_PASSWORD`.
Set `HOSTCTL_UPLOAD_TOKEN` when the target requires it. Hardware wrappers load
that file; direct `scripts/hostctl.sh` does not. Use a new `HOSTCTL_NET_LOG_PATH`
for standalone runs. Retain board and flashed-artifact identity with results.

```bash
scripts/tests/hw/test_wifi_acceptance.sh
```

The wrapper invokes `hostctl test wifi-acceptance`, using
[acceptance YAML](../../../tools/hostctl/scenarios/wifi-acceptance.sw.yaml) and
[policy defaults](../../../tools/hostctl/scenarios/wifi-policy.default.json).
Readiness comes from NET_STATUS structured frames. Classify `failure_class` /
`failure_code` before interpreting upload errors; retain the raw log and compare
cycle summaries after one targeted fix.

For Waveshare use `HOSTCTL_NET_TARGET=s3` with the wrapper, or direct
`test wifi-acceptance --target s3`. Its token must match the build. Serial open
resets the S3, and startup requires a fresh UI resource sample. S3 start ACK
follows network restoration; reports keep `start_to_ack_ms` separate from
post-ACK connection/listener polling. Resource checks require live LVGL integrity,
free-space/internal reserve, advancing uptime, and a ready listener.

## Discovery debug

```bash
HOSTCTL_NET_LOG_PATH=logs/<new-discovery-run>.log \
  scripts/tests/hw/test_wifi_discovery_debug.sh
```

[The TOML profile](../../../tools/hostctl/scenarios/wifi-discovery-debug.default.toml)
owns rounds/thresholds; [YAML](../../../tools/hostctl/scenarios/wifi-discovery-debug.sw.yaml)
owns orchestration. Default rounds temporarily disable the HTTP listener and
report empty/nonempty scans, no-AP events, and target SSID visibility.

## Regression gate

```bash
scripts/tests/hw/test_wifi_regression_gate.sh
```

Sequence: discovery → one-cycle acceptance → three-cycle acceptance → optional
soak. The gate is panic-first/fail-fast and retains stage logs plus `report.json`.
Panic/reset markers produce an excerpt and can invoke troubleshooting, which
normally reflashes; disable it when preserving diagnostic firmware.

| Variable | Default / purpose |
| --- | --- |
| `HOSTCTL_NET_SOAK_CYCLES` | `0`, optional additional acceptance cycles |
| `HOSTCTL_NET_PANIC_AUTO_TROUBLESHOOT` | `1` |
| `HOSTCTL_NET_REGRESSION_OUTPUT_DIR` | `logs/wifi_regression_gate_<timestamp>` |
| `HOSTCTL_NET_LOCK_WAIT_SEC` | `0`, fail-fast serial lock |
| `HOSTCTL_NET_ALLOW_LOG_APPEND` | `0`, unique evidence paths |
| `HOSTCTL_NET_RETAIN_RESET_CAPTURE_SECS` | Diagnostic failure watch; maximum `360` seconds |

For reset attribution, retain the existing discovery UART connection with
`HOSTCTL_NET_RETAIN_RESET_CAPTURE_SECS=120` and set auto-troubleshoot to `0`.
This changes capture only, never the failing verdict. Routine regression is the
one/three-cycle gate. Extended soak is explicitly cycle-based; record its actual
duration and coverage rather than claiming a 24-hour pass from cycle count.

## Inkplate Wi-Fi/BLE handoff

Use the default Meditamer release build and the Phase 1S workflow for exclusive
Wi-Fi/BLE handoff. The old Phase 1D `BLEPROBE` runner is incompatible with current
firmware. Other targets need their own integration and validation procedure.

Follow [host setup](../development/setup.md), then choose the device's serial port
and give the build a unique ID:

```bash
HOSTCTL_PORT="$DEVICE_PORT" \
HOSTCTL_FLASH_CAPTURE_LOG_PATH=logs/ble_phase1s_flash \
MEDITAMER_FIRMWARE_BUILD_ID=ble-p1s-001 \
scripts/device/flash.sh release
```

Export `HOSTCTL_NET_SSID` and `HOSTCTL_NET_PASSWORD` from a private local env file,
and `HOSTCTL_UPLOAD_TOKEN` when required. Run against those captured artifacts:

```bash
HOSTCTL_PORT="$DEVICE_PORT" \
HOSTCTL_BLE_TELEMETRY_PROFILE=minimal \
scripts/hostctl.sh test ble-phase1s \
  --artifacts logs/ble_phase1s_flash \
  --board-id inkplate4-tempera-01 --cycles 20
```

The runner checks build ID, release/default composition and ELF/application hashes.
It rejects extra features, disabled defaults and standalone binaries. An unconnected
20-cycle run takes at least 20 minutes.

`gate_passed` covers the runner's radio handoff, callback/queue cleanup, resource
checks and Wi-Fi/upload restoration. Limits live in the
[Inkplate memory budget](../../references/memory/meditamer-inkplate/budget.md#acceptance).
Run [memory validation](../memory/validation.md) on the same artifact for static guards.
A passing handoff run does not establish GATT interoperability or power compliance.

The report records build/source identity, hashes, telemetry profile and console
drops. `minimal` temporarily disables telemetry and restores the original mask;
restoration failure or new diagnostic drops fails the run. A minimal-profile pass
provides no evidence for verbose logging.
