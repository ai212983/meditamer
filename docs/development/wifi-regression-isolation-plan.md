# Wi-Fi Regression Isolation Plan

## Goal

Isolate the specific regression that turned the main no-std firmware from a
working Wi-Fi discovery/upload state into the current zero-discovery blackout.

This plan exists because:

- the main no-std firmware was passing discovery and upload gates on
  `2026-03-05`
- the blackout is first clearly recorded on `2026-03-06`
- official C/ESP-IDF and Rust-on-ESP-IDF controls work on the same board
- official ESP-IDF scan works with PSRAM both off and on
- deep legacy-port and true-old-stack work did not restore discovery on the
  current branch

The shortest credible path now is regression isolation from a known-good main
firmware baseline, not more backend invention on the broken branch.

## Current Evidence

### Last Known-Good Main Firmware Window

The last clean green window for the main no-std firmware is recorded in:

- [part-13 history](./upload-throughput-history/part-13.md)

Most relevant artifacts:

- `logs/wifi_regression_gate_20260305_145541/report.json`
- `logs/wifi_regression_gate_20260305_153302/report.json`
- `logs/wifi_regression_gate_20260305_165416/report.json`
- `logs/wifi_regression_gate_20260305_165808/report.json`

These are stronger than a scan-only proof. They show the main firmware could:

- produce scan evidence every round
- reach readiness
- complete upload acceptance cycles

### First Recorded Blackout Window

The first clean blackout record is in:

- [part-14 history](./upload-throughput-history/part-14.md)

Most relevant artifacts:

- `logs/wifi_acceptance_reason2fix_20260306_072339.log`
- `logs/wifi_discovery_reason2fix_20260306_072522.log`

Observed there:

- `failure_class=discovery_empty`
- all-zero scan paths
- no association progress because discovery never begins

### Tightened Fault Domain

The following have been proven separately and should not be re-litigated during
this plan:

- official C/ESP-IDF scan works
- Rust-on-ESP-IDF scan works
- official ESP-IDF scan works with PSRAM off
- official ESP-IDF scan works with PSRAM on
- standalone legacy no-std `esp-wifi 0.15.1` works on the same board

Therefore the remaining fault domain is:

- the current main no-std Wi-Fi integration path
- most likely in `esp-radio` / `esp-rtos` / backend integration
- not in board RF environment
- not in generic ESP-IDF Wi-Fi behavior
- not in PSRAM as the primary cause

## Rollback Anchor

Use this commit as the initial rollback baseline:

- `826b235504a6caeb38c6ae5e5e55e546e2531757`
- `fix(upload): harden acceptance startup listener recovery`

Why this anchor:

- it aligns with the last green [part-13.md](./upload-throughput-history/part-13.md) validation window
- it is a main-firmware state, not a standalone comparator
- it predates the first recorded blackout in [part-14.md](./upload-throughput-history/part-14.md)

## Validation Rules

Use only these validation methods on the rollback/isolation branch:

- bounded discovery regression gate
- canonical boot-scan validation
- acceptance gate only after discovery is green again

Required evidence when a step is considered green:

- scan evidence every round in the bounded discovery gate
- non-zero AP discovery or equivalent scan evidence markers
- no `failure_class=discovery_empty`

When a step is considered bad:

- discovery gate falls into all-zero scan behavior
- canonical boot-scan shows:
  - pre-scan promisc `0`
  - `wifi_rx_cb_count sta=0 ap=0`
  - direct explicit scan `ap_num=0`

## Non-Goals

Do not do any of the following while executing this plan:

- more legacy-port work
- more true-old-stack work
- more blob-compat extraction work
- more PSRAM-focused experiments
- broad branch refactors unrelated to identifying the first bad slice

## Phase Checklist

- [ ] Phase 1: recreate the known-good baseline on a fresh branch
- [ ] Phase 2: revalidate the baseline on hardware
- [ ] Phase 3: define replay batches after the good anchor
- [ ] Phase 4: replay non-Wi-Fi-safe batches
- [ ] Phase 5: replay Wi-Fi-adjacent batches in narrow slices
- [ ] Phase 6: identify the first bad slice
- [ ] Phase 7: reduce the first bad slice to a minimal offending change set
- [ ] Phase 8: choose stabilize-via-revert or forward-fix

## Phase 1: Recreate The Known-Good Baseline

### Goal

Create a clean working branch from the last known-good main-firmware anchor.

### Steps

- [ ] Step 1.1 create a new branch from `826b235`
- [ ] Step 1.2 preserve the current `fix/wifi_connectivity` branch as the
      broken control branch
- [ ] Step 1.3 document the baseline branch name and anchor commit

### Deliverable

A dedicated regression-isolation branch rooted at the last known-good main
firmware state.

## Phase 2: Revalidate The Known-Good Baseline

### Goal

Confirm the historical green state still reproduces on current hardware.

### Steps

- [ ] Step 2.1 run the bounded discovery gate on the rollback branch
- [ ] Step 2.2 run one canonical boot-scan capture
- [ ] Step 2.3 classify the branch as:
  - still good
  - not reproducible anymore

### Decision Rule

If the rollback branch is no longer good on current hardware, stop replay work.
At that point the regression is not explainable as a simple code regression
between `2026-03-05` and `2026-03-06`, and the plan must be rewritten around
environment drift.

## Phase 3: Define Replay Batches

### Goal

Replay post-anchor work in batches that are technically meaningful and easy to
bisect.

### Batch Types

#### Batch A: Host-Only / Tooling-Only

Examples:

- `tools/hostctl/`
- `scripts/`
- host workflow/test files

These should not change device-side discovery behavior. Replay them first to
prove the gate infrastructure itself is not the cause.

#### Batch B: App Refactors With No Wi-Fi Runtime Behavior Change

Examples:

- concern-splitting refactors outside Wi-Fi runtime
- SD/upload/internal telemetry refactors that should not alter Wi-Fi driver
  state

Replay only after Batch A is green.

#### Batch C: Wi-Fi App Logic Changes

Examples:

- `src/firmware/storage/upload/wifi/`
- discovery policy
- connection recovery logic
- scan handling
- promisc diagnostics

These are the first likely regression candidates.

#### Batch D: Vendored Runtime / Radio Changes

Examples:

- `vendor/esp-radio-0.17.0/`
- `vendor/esp-rtos-0.2.0/`

Treat this as the highest-risk batch. Replay it last and in the smallest
possible slices.

### Steps

- [ ] Step 3.1 build a file-domain map from `826b235..HEAD`
- [ ] Step 3.2 assign each post-anchor commit to Batch A, B, C, or D
- [ ] Step 3.3 write the initial replay order into this plan


_Continue with [phases 4–8 and stop conditions](./wifi-regression-isolation-plan/part-02.md)._
