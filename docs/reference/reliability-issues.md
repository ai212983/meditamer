# Reliability Issues (Current)

As of: 2026-08-13

Primary audience: LLM agents and automated workflow runners.
Secondary audience: human developers.

## Scope

This document tracks current reliability risks observed in the firmware/runtime + host harness stack.
It is ordered by operational impact.

## REL-001: True Cold-Boot Reliability Is Not Validated

- Severity: high
- Status: mitigated — reset-cycle evidence accepted; true power-rail cold boot remains unvalidated
- Impact:
  - The Inkplate 4 TEMPERA has a built-in, non-removable battery and no on/off power switch;
    confirmed against hardware in hand, the only physical control besides USB is a reset button.
    A reset-button press exercises the ESP32 boot sequence but does not necessarily cut power to
    peripherals whose rails aren't gated by that button (e.g. the e-ink PMIC, SD rail), so it is a
    narrower guarantee than a genuine power-rail cold boot. That gap remains open.
- Evidence:
  - `scripts/device/cold_boot_matrix.sh`, run 2026-08-13 against the final integrated artifact
    (source HEAD `803077a0816b`, durable snapshot commit `dc7178eda5f0`, device
    `/dev/cu.usbserial-2110`): **5/5 reset cycles passed** — `BOOT_RESET reason=`,
    `touch: ready phase=`, `LVGL init=ready`, and `RUNTIME_READY app_state=ready display=ready` all
    present in every cycle; each log 99.99-100% printable ASCII (no binary/noise-only captures).
    [Cold-boot validation](../plans/cold-boot-validation.md) records the hardware finding, exact
    artifact identity, per-cycle hashes, and accepted-evidence decision.
- Mitigation path (done): the reset-button boot-path matrix is accepted as REL-001's evidence given
  the hardware constraint; 5/5 cycles passed and are archived.
- Residual gap: a true power-rail cold boot (LED fully off / battery physically disconnected) has
  still never been demonstrated on this device. Revisit only if a method becomes available (e.g.
  disassembly to the internal battery connector) or if reset-cycle evidence proves insufficient in
  practice (e.g. a bug surfaces that only manifests after a genuine power loss).
- Acceptance criteria (met): 5/5 reset cycles pass with required markers and no binary/noise-only
  captures, explicitly labeled as reset-cycle (not power-rail cold-boot) evidence.

## REL-002: Wi-Fi Association/Reachability Still Needs Recovery Workarounds

- Severity: high
- Status: partially mitigated
- Impact:
  - Upload runs can still enter states where health/reachability requires mode-cycling or reset recovery.
  - This increases test/runtime variance and failure probability.
- Evidence:
  - Regression harness documents explicit recovery behavior:
    - mode-cycle recovery [docs/guides/wifi-asset-upload.md](../guides/wifi-asset-upload.md)
    - reset fallback when mode recovery does not ACK [docs/guides/wifi-asset-upload.md](../guides/wifi-asset-upload.md)
  - Wi-Fi task contains extensive reassociation/scan/auth/channel fallback logic:
    - [src/firmware/net/wifi.rs](../../src/firmware/net/wifi.rs)
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
    - [docs/guides/development-setup.md](../guides/development-setup.md)
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
    - [src/firmware/serial.rs](../../src/firmware/serial.rs)
    - [src/firmware/serial.rs](../../src/firmware/serial.rs)
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
    - [docs/guides/development-setup.md](../guides/development-setup.md)
  - Current hotspots exceed limits (from local scan):
    - `src/firmware/observability.rs` (~848)
    - `src/firmware/net/wifi.rs` (~764)
    - `src/firmware/serial.rs` (~577)
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
    - [docs/archive/upload/upload-throughput-history.md](../archive/upload/upload-throughput-history.md)
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
    - [docs/guides/wifi-asset-upload.md](../guides/wifi-asset-upload.md)
  - Wi-Fi credentials are stored in plaintext key-value form:
    - [src/firmware/storage/sd_task/wifi_config.rs](../../src/firmware/storage/sd_task/wifi_config.rs)
    - [src/firmware/storage/sd_task/wifi_config.rs](../../src/firmware/storage/sd_task/wifi_config.rs)
- Mitigation path:
  - Enforce token in non-dev builds by default.
  - Add clear deployment profile guidance and optional credential-protection strategy.
- Acceptance criteria:
  - Non-dev deployment path prevents unauthenticated mutating operations.

## REL-008: Zero-Discovery Regressions from Scan/Orchestration Timing Drift

- Severity: high
- Status: partially mitigated
- Impact:
  - Discovery can regress to zero AP visibility and block acceptance/throughput runs.
  - False negatives can be introduced by host workflow timing/command pressure.
- Evidence:
  - Dedicated root-cause and guardrails note:
    - [docs/archive/wifi/wifi-discovery-regression-guardrails.md](../archive/wifi/wifi-discovery-regression-guardrails.md)
  - Discovery-debug and acceptance flow now explicitly separate discovery proof from throughput profiling:
    - [docs/guides/wifi-regression-gate.md](../guides/wifi-regression-gate.md)
- Mitigation path:
  - Keep `wifi-discovery-debug` first after boot and require non-zero scan + SSID visibility.
  - Preserve timeout shaping aligned to scan dwell and recovery budgets.
  - Enforce single-workflow-per-device and unique per-run logs.
- Acceptance criteria:
  - Repeated discovery-debug runs show zero zero-discovery rounds under normal AP conditions.
  - Acceptance (1-cycle, 3-cycle, bounded soak) remains stable after discovery proof.

## Suggested Next Execution Order

1. Reduce REL-002 fallbacks needed in normal Wi-Fi conditions while preserving the REL-008 discovery gate.
2. Address REL-003 by adding host-runnable automated regression tests.
3. Fix REL-004 overflow observability.
4. Execute REL-005 splits while preserving behavior.
