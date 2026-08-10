# Wi-Fi Discovery Regression Guardrails

As of: 2026-03-03

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

Canonical single-command path:

- run `scripts/tests/hw/test_wifi_regression_gate.sh`
- this wraps the sequence above, writes per-stage logs, and emits `report.json`
- panic/reboot signatures are treated as hard failure signals and trigger panic excerpt capture

## Panic-First Handling

When a panic or unexpected reboot signature appears (`Guru Meditation`, `panic`, `backtrace`,
`stack overflow`, `stack smashing`, `assertion failed`, `BOOT_RESET reason=...`):

1. Stop concurrent workflows for the same device/port.
2. Preserve failing logs and panic excerpt.
3. Run one troubleshoot pass (`scripts/tests/hw/test_troubleshoot_hw.sh`).
4. Reproduce once with unchanged settings.
5. If reproduced twice, open blocking regression report with artifact bundle.

Full operational protocol (reporting template, triage ladder, closure criteria):

- `docs/development/wifi-upload-regression-protocol.md`

## Throughput Diagnostic Constraint

When profiling upload throughput, do not continue long upload runs if discovery/connectivity is failing.
Discovery non-zero is a prerequisite for meaningful upload-rate diagnostics.

After discovery has already been proven non-zero in the current session (discovery-debug + acceptance smoke),
run pure throughput profiling with:

- `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`

Rationale: avoids spending upload diagnostics time in strict boot scan-evidence gating loops.

## Maintenance Notes

- Preserve and extend comments around scan timeout shaping and esp-radio dwell semantics in:
  - `tools/hostctl/src/workflows_wifi_discovery.rs`
- If refactoring acceptance/discovery workflows, keep this invariant:
  - discovery proof first, throughput profiling second.

## Latest Verification (2026-03-03)

Pipeline A/B regression gate runs (`off` vs `on`) both passed discovery and acceptance stages:

- `logs/wifi_regression_gate_ab_off_20260303_115711/report.json`
- `logs/wifi_regression_gate_ab_on_20260303_120041/report.json`
- `logs/wifi_regression_gate_default_confirm_20260303_121014/report.json` (post-enable default confirmation)
- `logs/wifi_regression_gate_chunk_ab_49152_final_20260303_123948/report.json` (`SD_UPLOAD_CHUNK_MAX=49_152`)
- `logs/wifi_regression_gate_chunk_ab_65536_20260303_124328/report.json` (`SD_UPLOAD_CHUNK_MAX=65_536`)
- `logs/wifi_regression_gate_chunk_ab_65536_soak10_clean_20260303_130052/report.json` (`SD_UPLOAD_CHUNK_MAX=65_536`, soak stage failed with runtime panic)
- `logs/wifi_regression_gate_65536_postfix_20260303_141406/report.json` (`SD_UPLOAD_CHUNK_MAX=65_536`, post-mitigation soak stage passed)
- `logs/wifi_regression_gate_default65536_connresetfix_r1_20260303_144611/report.json` (post transport-reset hardening, default `65_536`, soak stage passed)
- `logs/wifi_regression_gate_default65536_connresetfix_r2_20260303_144943/report.json` (post transport-reset hardening, default `65_536`, soak stage passed)
- `logs/wifi_regression_gate_default65536_connresetfix_r3_20260303_145315/report.json` (post transport-reset hardening, default `65_536`, soak stage passed)
- `logs/wifi_regression_gate_sdspi36b_20260303_151750/report.json` (`MEDITAMER_SD_SPI_DATA_MHZ=36`, full gate + soak passed)
- `logs/wifi_regression_gate_sdspi40_20260303_152151/report.json` (`MEDITAMER_SD_SPI_DATA_MHZ=40`, full gate + soak passed)
- `logs/wifi_regression_gate_sdspi36_appenddiag_r1b_20260303_163755/report.json` (`36 MHz`, append-path diagnostics enabled, full gate + soak passed)
- `logs/wifi_regression_gate_sdspi36_appenddiag_r2_20260303_164229/report.json` (`36 MHz`, append-path diagnostics enabled, full gate + soak passed)
- `logs/wifi_regression_gate_sdspi36_appenddiag_r3_20260303_164631/report.json` (`36 MHz`, append-path diagnostics enabled, full gate + soak passed)
- `logs/wifi_regression_gate_sdspi36_queuebridge_r1_20260303_170129/report.json` (`36 MHz`, queue-boundary diagnostics enabled, full gate + soak passed)
- `logs/wifi_regression_gate_sdspi36_queuebridge_r2b_20260303_170808/report.json` (`36 MHz`, queue-boundary diagnostics enabled, full gate + soak passed)
- `logs/wifi_regression_gate_sdspi36_queuebridge_r3_20260303_171241/report.json` (`36 MHz`, queue-boundary diagnostics enabled, full gate + soak passed)

Observed discovery outcome in all listed runs:

- `zero_discovery_rounds == 0`
- `scan_nonzero_events > 0`
- `ssid_seen_rounds > 0`

Conclusion:

- Enabling upload chunk pipeline by default did not reintroduce the zero-discovery regression signature in this validation pass.
- Three additional post-hardening reruns (`connresetfix_r1..r3`) also preserved the same discovery invariants with full soak-stage passes.
- SD-SPI variance A/B reruns (`36` vs `40`) likewise preserved discovery invariants while throughput-tail behavior diverged.
