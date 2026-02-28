# Reliability Issues (Current)

As of: 2026-02-27

Primary audience: LLM agents and automated workflow runners.
Secondary audience: human developers.

## Scope

This document tracks current reliability risks observed in the firmware/runtime + host harness stack.
It is ordered by operational impact.

## REL-001: True Cold-Boot Reliability Is Not Validated

- Severity: high
- Status: open
- Impact:
  - We do not yet have confidence that real power-cycle behavior is stable.
  - USB unplug/replug can produce false confidence because the board may stay powered from battery.
- Evidence:
  - [docs/todos/cold-boot-validation.md](../todos/cold-boot-validation.md) marks this as deferred.
  - [docs/todos/cold-boot-validation.md](../todos/cold-boot-validation.md) explains battery-backed power persistence.
  - [docs/todos/cold-boot-validation.md](../todos/cold-boot-validation.md) requires 5/5 true cold-boot passes.
- Mitigation path:
  - Establish repeatable true-off procedure (LED off or battery disconnect).
  - Run manual cold-boot matrix and archive logs/results.
- Acceptance criteria:
  - 5/5 true cold-boot cycles pass with required markers and no binary/noise-only captures.

## REL-002: Wi-Fi Association/Reachability Still Needs Recovery Workarounds

- Severity: high
- Status: partially mitigated
- Impact:
  - Upload runs can still enter states where health/reachability requires mode-cycling or reset recovery.
  - This increases test/runtime variance and failure probability.
- Evidence:
  - Regression harness documents explicit recovery behavior:
    - mode-cycle recovery [docs/development/README.md](README.md)
    - reset fallback when mode recovery does not ACK [docs/development/README.md](README.md)
  - Wi-Fi task contains extensive reassociation/scan/auth/channel fallback logic:
    - [src/firmware/storage/upload/wifi.rs](../../src/firmware/storage/upload/wifi.rs)
- Mitigation path:
  - Continue instrumentation around association stages and DHCP/listener transitions.
  - Reduce need for host-driven recovery by tightening in-firmware state transitions.
- Acceptance criteria:
  - Multi-cycle upload regression passes without mode-cycle/reset fallback in normal AP conditions.

## REL-003: Behavior-Level Automated Test Coverage Is Thin

- Severity: high
- Status: open
- Impact:
  - Regressions can slip through when only style/lint checks run.
  - Reliability validation depends heavily on manual/device scripts.
- Evidence:
  - `cargo test --features asset-upload-http` currently runs doc-tests with zero tests executed.
  - Test harness defaults are disabled in manifest:
    - [Cargo.toml](../../Cargo.toml)
    - [Cargo.toml](../../Cargo.toml)
  - Hooks emphasize fmt/clippy/link checks:
    - [docs/development/README.md](README.md)
    - [docs/development/README.md](README.md)
- Mitigation path:
  - Add host-runnable parser/protocol/state-machine tests that do not require target execution.
  - Add CI gates for key regression scripts where feasible.
- Acceptance criteria:
  - Non-trivial automated tests execute in CI for parser/protocol/runtime-control flows.

## REL-004: UART Command Buffer Overflow Is Not Explicitly Reported

- Severity: medium
- Status: open
- Impact:
  - Oversized command input can be dropped silently, creating hard-to-diagnose host behavior.
- Evidence:
  - In serial task read loop, overflow resets buffer (`line_len = 0`) without returning an explicit overflow error:
    - [src/firmware/runtime/serial_task.rs](../../src/firmware/runtime/serial_task.rs)
    - [src/firmware/runtime/serial_task.rs](../../src/firmware/runtime/serial_task.rs)
- Mitigation path:
  - Return a dedicated error response (`CMD ERR reason=overflow`) when overflow is detected.
- Acceptance criteria:
  - Host can deterministically distinguish syntax errors from overflow errors.

## REL-005: File Size/Complexity Hotspots Increase Regression Risk

- Severity: medium
- Status: open
- Impact:
  - Large files make review, test targeting, and safe edits harder.
- Evidence:
  - Guideline says hard cap 500 LOC, split trigger at 420:
    - [docs/development/README.md](README.md)
    - [docs/development/README.md](README.md)
  - Current hotspots exceed limits (from local scan):
    - `src/firmware/telemetry/mod.rs` (~848)
    - `src/firmware/storage/upload/wifi.rs` (~764)
    - `src/firmware/runtime/serial_task.rs` (~577)
- Mitigation path:
  - Split by responsibility: parser/protocol, metrics formatting, wifi scan/reassoc policy, mode transitions.
- Acceptance criteria:
  - Hotspot files reduced below guideline thresholds, with no behavior changes.

## REL-006: Upload Throughput Remains Low/Variable (Timeout Pressure Risk)

- Severity: medium
- Status: partially mitigated
- Impact:
  - Low/variable throughput stretches operation windows and increases timeout/recovery exposure.
- Evidence:
  - Historical data still shows low single-digit KiB/s and variance by payload:
    - [docs/development/upload-throughput-history.md](upload-throughput-history.md)
    - [docs/development/upload-throughput-history.md](upload-throughput-history.md)
    - [docs/development/upload-throughput-history.md](upload-throughput-history.md)
- Mitigation path:
  - Keep per-phase telemetry comparisons and optimize highest-time bucket first.
- Acceptance criteria:
  - Stable throughput target met across repeated runs with narrow variance.

## REL-007: Security/Ops Gaps That Can Affect Reliability in Shared Environments

- Severity: medium
- Status: open
- Impact:
  - Open mutating endpoints (when token unset) and plaintext Wi-Fi config can cause accidental or hostile interference.
  - Interference can present as reliability instability.
- Evidence:
  - Upload endpoint auth can be disabled if no token is configured:
    - [docs/development/README.md](README.md)
  - Wi-Fi credentials are stored in plaintext key-value form:
    - [src/firmware/storage/sd_task/wifi_config.rs](../../src/firmware/storage/sd_task/wifi_config.rs)
    - [src/firmware/storage/sd_task/wifi_config.rs](../../src/firmware/storage/sd_task/wifi_config.rs)
- Mitigation path:
  - Enforce token in non-dev builds by default.
  - Add clear deployment profile guidance and optional credential-protection strategy.
- Acceptance criteria:
  - Non-dev deployment path prevents unauthenticated mutating operations.

## Suggested Next Execution Order

1. Close REL-001 (true cold-boot validation).
2. Reduce REL-002 fallbacks needed in normal Wi-Fi conditions.
3. Address REL-003 by adding host-runnable automated regression tests.
4. Fix REL-004 overflow observability.
5. Execute REL-005 splits while preserving behavior.
