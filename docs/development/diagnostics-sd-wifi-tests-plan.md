# Diagnostics SD+WiFi Tests Plan

## Objective

Implement executable diagnostics sessions for `SD` and `WIFI` under the centralized app-state machine introduced by the state refactor.

## Scope

- In scope:
  - `STATE DIAG kind=test targets=...` execution for `SD`, `WIFI`, and `SD|WIFI`.
  - Deterministic session lifecycle (`queued -> running -> done/failed/canceled`).
  - UART status/report output for automation.
  - Explicit resource arbitration with upload/runtime networking paths.
- Out of scope:
  - New hardware probes beyond existing SD/WiFi primitives.
  - UI redesign for diagnostics views.

## State/Control Contract

1. Enter diagnostics:
   - `STATE DIAG kind=test targets=SD|WIFI` transitions app phase to `DIAGNOSTICS_EXCLUSIVE`.
2. Exit diagnostics:
   - `STATE DIAG kind=none targets=` returns to `OPERATING`.
3. While diagnostics active:
   - Normal upload/session flows are gated off.
   - `STATE GET` includes diag status and targets.

## Execution Model

1. Add `diagnostics_task` with bounded command/result channels.
2. App-state engine emits `StartDiagSession { kind, targets }` action on accepted transition.
3. Diagnostics task executes selected targets:
   - `SD`: probe + rw verify + optional FAT stat in configured path.
   - `WIFI`: start/connect health checks using existing Wi-Fi task capabilities.
4. Combined `SD|WIFI` runs sequentially by default (SD then WIFI) to reduce contention.

## UART/API Additions

1. New status line:
   - `DIAG state=<idle|running|done|failed> targets=<...> step=<...> code=<...>`
2. Optional pull command:
   - `DIAG GET`
3. Existing `STATE GET` remains source-of-truth for mode/phase.

## Failure Policy

1. Any target failure marks session `failed`.
2. Session failure does not panic firmware; returns explicit error code.
3. On failure, app remains in diagnostics phase until explicit `STATE DIAG kind=none`.

## Tests

1. Host parser tests:
   - parse `STATE DIAG ...` combinations and invalid tokens.
2. App-state tests:
   - diagnostics enter/exit transitions and invariant enforcement.
3. Diagnostics task tests:
   - SD-only success/failure mapping.
   - WIFI-only success/failure mapping.
   - combined sequential execution order and final aggregation.
4. Hardware validation:
   - run diagnostics over UART and verify deterministic status/report lines.

## Acceptance Criteria

1. `STATE DIAG` starts real SD/WiFi diagnostics (not scaffolding only).
2. Combined `SD|WIFI` session is supported and deterministic.
3. Diagnostics do not interfere with normal upload path after exit.
4. Failure/success signals are machine-parseable over UART.
