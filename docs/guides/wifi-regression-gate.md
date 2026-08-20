# Wi-Fi Regression Gate

The Wi-Fi zero-discovery blackout is fixed. This gate is what keeps it fixed;
run it before landing changes that touch Wi-Fi, the network stack, or upload.

The blackout-era instrumentation used to find the original fault is archived in
[docs/archive/wifi/blackout-diagnostic-knobs.md](../archive/wifi/blackout-diagnostic-knobs.md).
The upload server the gate exercises is documented in
[SD Asset Upload Over Wi-Fi](wifi-asset-upload.md); the acceptance workflow's
env and UART contracts are in
[Network Acceptance Workflow](agents/network-acceptance.md).

## Discovery debug

Wi-Fi zero-discovery diagnostic workflow:

```bash
HOSTCTL_NET_PORT=/dev/cu.usbserial-540 \
HOSTCTL_NET_BAUD=115200 \
HOSTCTL_NET_SSID='<wifi-ssid>' \
HOSTCTL_NET_PASSWORD='<wifi-password>' \
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
- root-cause and guardrails reference:
  `docs/archive/wifi/wifi-discovery-regression-guardrails.md`.

## Regression gate

Wi-Fi/upload regression gate (panic-first, fail-fast):

```bash
scripts/tests/hw/test_wifi_regression_gate.sh
```

- sequence: discovery debug -> acceptance 1-cycle -> acceptance 3-cycle -> optional soak
- emits per-stage logs and machine-readable `report.json`
- when panic/reboot markers are detected, the gate captures panic excerpt and can auto-run troubleshoot workflow

Optional regression-gate env vars:

- `HOSTCTL_NET_SOAK_CYCLES` (`0` default; skip soak)
- `HOSTCTL_NET_PANIC_AUTO_TROUBLESHOOT` (`1` default)
- `HOSTCTL_NET_REGRESSION_OUTPUT_DIR` (default `logs/wifi_regression_gate_<timestamp>`)

Wi-Fi workflow guardrail env vars:

- `HOSTCTL_NET_LOCK_WAIT_SEC` (`0` default; fail-fast lock)
- `HOSTCTL_NET_ALLOW_LOG_APPEND` (`0` default; enforce unique log path)
- `HOSTCTL_EXPERIMENT_NOVELTY_GUARD` (`1` default; set `0` to bypass decision-ledger guard)
- `HOSTCTL_EXPERIMENT_NOVELTY_OVERRIDE` (`0` default; set `1` to allow intentional reruns of already-decided knobs)
