# Wi-Fi/Upload Regression Protocol (Panic-First)

As of: 2026-03-03

## Purpose

Define deterministic handling for Wi-Fi/upload regressions, with stack panic and unexpected reboot treated as highest-priority signals.

## Trigger Conditions

Open a regression incident when any condition is true:

1. `wifi-discovery-debug` reports `zero_discovery_rounds > 0`.
2. Wi-Fi acceptance fails in 1-cycle or 3-cycle run.
3. Upload `/health` fails after configured retries.
4. Panic/reboot signatures are detected in stage logs:
   - `Guru Meditation`
   - `panic`
   - `backtrace`
   - `stack overflow`
   - `stack smashing`
   - `assertion failed`
   - `BOOT_RESET reason=...` mid-run
5. Failure reproduces in two consecutive clean runs.

## Immediate Containment

1. Stop all concurrent host workflows on the same device/port.
2. Preserve the failing log files as immutable artifacts.
3. Capture first panic marker (if present) and panic context excerpt.
4. Run one troubleshooting pass:

```bash
HOSTCTL_PORT=<serial-port> scripts/tests/hw/test_troubleshoot_hw.sh
```

5. Re-run the same failing scenario once with unchanged settings.

## Canonical Regression Gate

Run:

```bash
scripts/tests/hw/test_wifi_regression_gate.sh
```

This executes:

1. `wifi-discovery-debug` bounded
2. acceptance 1-cycle
3. acceptance 3-cycle
4. optional soak (`HOSTCTL_NET_SOAK_CYCLES`)

Outputs:

- stage logs
- panic excerpt (if detected)
- `report.json` summary artifact

## Classification Protocol

Primary source is panic/regression markers + structured `NET_STATUS`.

Standard classes:

- `runtime_panic_guru`
- `runtime_panic_stack`
- `runtime_panic_assert`
- `runtime_panic_other`
- `runtime_unexpected_reboot`
- `discovery_zero`
- `dhcp_no_ipv4_stall`
- `listener_unreachable`
- `health_unreachable`
- `upload_http_error`
- `uart_transport`
- `orchestration_timeout_mismatch`
- `unknown`

If panic class is present, it takes precedence over downstream timeout symptoms.

## Required Artifact Bundle

Attach all of:

1. `report.json` from regression gate
2. discovery log
3. acceptance 1-cycle log
4. acceptance 3-cycle log (or explicit stage failure reason)
5. soak log when run
6. panic excerpt file (if panic detected)
7. troubleshoot log path and summary
8. command/env overrides used (credentials/tokens redacted)
9. commit SHA + dirty/clean state

## Panic Triage Ladder

1. Locate first panic marker line and classify it.
2. Check whether `BOOT_RESET reason=...` appears after stage start.
3. Compare `METRICS BOOT reset_code` before and after run when available.
4. Ignore secondary command-timeout noise until first panic marker is explained.
5. Apply one targeted fix only, then rerun full gate.

## Reporting Template Fields

Every regression report must include:

1. first seen timestamp
2. run ID
3. failing stage
4. failure class/code
5. panic class (nullable)
6. panic marker line/index (nullable)
7. panic excerpt path (nullable)
8. reset code before/after (nullable)
9. reproducibility count
10. fix hypothesis and next single fix target

## Closure Criteria

Close an incident only when all hold:

1. discovery bounded run passes with required counters:
   - `zero_discovery_rounds == 0`
   - `scan_nonzero_events > 0`
   - `ssid_seen_rounds > 0`
2. acceptance 1-cycle passes
3. acceptance 3-cycle passes
4. if recovery/panic path changed, bounded soak passes
5. final report links fix commit + validation artifacts

## Enforcement Phases

Soft-now:

- host CI enforces host tests and stack-risk checks
- hardware evidence is mandatory in process/docs

Strict-later:

- after stable operation window, no merge for Wi-Fi/upload touching changes without regression artifact bundle
