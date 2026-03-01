# Wi-Fi Discovery Regression Guardrails

As of: 2026-03-01

## Problem Statement

We saw repeated regressions where discovery rounds reported zero APs (`scan_done count=0`) and acceptance runs failed even though the AP and firmware image were unchanged.

This note captures the verified cause, what was ruled out, and the guardrails that must remain in place.

## What Failed

- `wifi-discovery-debug` produced zero-result scan rounds.
- acceptance runs failed early at boot discovery/readiness gates.
- failures appeared intermittent and were harder to diagnose when multiple workflows wrote to the same log file.

## Verified Cause (This Incident)

The regression was primarily orchestration/timing pressure, not a persistent radio memory collapse:

1. Host-side timing/command pressure
- aggressive or overlapping control loops (`NET RECOVER`, `NET START`, listener toggles) can interrupt scan settling and push discovery into repeated zero-result windows.

2. Inadequate isolation during probe runs
- concurrent workflows and shared logs made state transitions look contradictory and triggered extra recovery commands.

3. Timeout-budget mismatch risk
- if host round timeouts are shorter than full scan/recovery budgets, healthy in-progress scans can be misclassified as "zero discovery."

## What Was Ruled Out

- No evidence that this specific zero-discovery recurrence was caused by immediate `NoMem` radio allocation failures.
- Discovery recovered to non-zero under isolated debug conditions and then passed acceptance (`1-cycle`, `3-cycle`, bounded soak), indicating a recoverable sequence/pressure issue.

## Required Guardrails

1. Run one host workflow at a time per device/port.
2. Always use a unique `HOSTCTL_NET_LOG_PATH` per run.
3. Keep discovery debug round timeout aligned with scan/recovery budget logic in hostctl (`tools/hostctl/src/workflows_wifi_discovery.rs`).
4. During probe rounds, keep HTTP listener disabled (`disable_listener_during_probe_rounds=true`) to reduce pressure while measuring discovery.
5. Do not lower scan dwell settings below policy defaults without re-validating discovery:
- `scan_active_min_ms=600`
- `scan_active_max_ms=1500`
- `scan_passive_ms=1500`
6. Keep post-recover settle conservative in discovery workflows:
- `recover_settle_ms=6000` (lower values, especially `1200`, have reintroduced immediate `discovery_empty` loops)
7. Scope acceptance boot-discovery gate to immediate post-boot only:
- `HOSTCTL_NET_BOOT_DISCOVERY_MAX_UPTIME_MS=30000` default
- rationale: avoid forcing scan-evidence checks during later cycles where link can remain healthy without new scan telemetry
8. Treat zero-discovery as a hard regression signal:
- require at least one non-zero scan event and at least one target SSID-seen round before acceptance throughput tests.

## Required Validation Sequence

Run in this order after boot:

1. Discovery debug (bounded)
- run `scripts/tests/hw/test_wifi_discovery_debug.sh`
- require:
  - `zero_discovery_rounds == 0`
  - `scan_nonzero_events > 0`
  - `ssid_seen_rounds > 0`

2. Wi-Fi acceptance 1-cycle
- run `scripts/tests/hw/test_wifi_acceptance.sh` with `HOSTCTL_NET_ACCEPTANCE_CYCLES=1`

3. Wi-Fi acceptance 3-cycle
- run `scripts/tests/hw/test_wifi_acceptance.sh` with `HOSTCTL_NET_ACCEPTANCE_CYCLES=3`

4. Bounded soak
- run `scripts/tests/hw/test_wifi_acceptance.sh` with a bounded soak cycle count (for example `10`).

## Throughput Diagnostic Constraint

When profiling upload throughput, do not continue long upload runs if discovery/connectivity is failing.
Discovery non-zero is a prerequisite for meaningful upload-rate diagnostics.

## Maintenance Notes

- Preserve and extend comments around scan timeout shaping and esp-radio dwell semantics in:
  - `tools/hostctl/src/workflows_wifi_discovery.rs`
- If refactoring acceptance/discovery workflows, keep this invariant:
  - discovery proof first, throughput profiling second.
