# Wi-Fi Legacy Vendoring Plan

## Purpose

This document is the source of truth for the Wi-Fi migration path after the
`esp-radio` / `esp-wifi-sys 0.8.1` RX-delivery investigation reached diminishing
returns.

Use this file to prevent strategic drift back into ad hoc hook A/B work.

## Motivation

The current boundary is already tight:

- working legacy no-std path (`esp-wifi 0.15.1` / `esp-wifi-sys 0.7.1`) sees raw
  packets and scans successfully
- current no-std path (`esp-radio` / `esp-wifi-sys 0.8.1`) keeps:
  - live `WIFI_MAC` interrupts
  - zero raw promiscuous callback delivery
  - zero internal RX callback delivery
  - zero scan admission
- wrapper, scan-config, timer, scheduler, task-entry, task-ring, and most
  blob-facing ISR wrapper branches have already been tested and logged

The practical conclusion is:

- root-cause tightening has been useful
- further narrow hook tweaking is now low-value
- the next engineering path is a true source-level legacy runtime/backend port

## Decision

Prefer wholesale vendoring/porting of the working legacy Wi-Fi runtime behavior
behind `backend_legacy_port` over additional isolated `esp-radio` hook
experiments.

## Scope

The port target is:

- legacy runtime/bootstrap behavior needed for Wi-Fi bring-up
- legacy timer/preempt/runtime behavior needed for packet delivery
- legacy Wi-Fi init/start/scan/stop behavior behind the existing backend seam

The port target is not:

- a full firmware runtime rollback
- downgrading the whole firmware crate graph
- continued broad A/B exploration of unrelated wrapper hooks

## Non-Goals

- do not downgrade `xtensa-lx-rt` for the main firmware
- do not add more optional old-dependency feature branches to the main crate
- do not treat standalone hook wins as durable until they are carried through
  `backend_legacy_port`

## Execution Rule

After this document exists, new Wi-Fi root-cause work should satisfy at least
one of these conditions:

1. it directly advances `backend_legacy_port`
2. it ports legacy runtime/source behavior into vendored runtime support
3. it validates the current `backend_legacy_port` path end to end

If a change does not satisfy one of those conditions, it is drift and should not
be started by default.

## Plan

### Phase 1: Backend Boundary

Goal:
- keep firmware-side backend ownership clean

Status:
- in progress

Tasks:
- keep `backend.rs` as a dispatcher only
- keep `backend_esp_radio.rs` pure to the current backend
- keep `backend_legacy_port/` as the only location for legacy-path behavior

### Phase 2: Runtime Port

Goal:
- port legacy runtime/bootstrap behavior into vendored runtime support

Status:
- in progress

Tasks:
- port legacy thread-semaphore behavior
- port legacy timer/preempt behavior
- port legacy scheduler/task model only where required by Wi-Fi runtime
- remove dependence on scattered diag-only runtime shims

### Phase 3: Wi-Fi Bring-Up Port

Goal:
- make `backend_legacy_port` own a coherent legacy-style Wi-Fi init/start/scan
  path

Status:
- in progress

Tasks:
- keep runtime init inside `backend_legacy_port/runtime.rs`
- keep controller start/scan/stop inside `backend_legacy_port/controller.rs`
- port the next lower packet-delivery path only through this backend

### Phase 4: Validation

Goal:
- validate only the backend path that is intended to survive

Status:
- pending

Tasks:
- validate `backend_legacy_port` through main firmware boot-scan
- confirm:
  - non-zero raw packet visibility
  - non-zero direct IDF scan
  - non-zero wrapped scan
- only then resume higher upload/discovery acceptance work

## Allowed Narrow Diagnostics

Narrow diagnostics are still allowed only if they directly answer a question
needed by the vendoring plan. For example:

- identifying the exact stall point inside `backend_legacy_port`
- proving whether a vendored legacy runtime slice changed packet delivery
- validating whether a ported legacy behavior survived in firmware

They are not allowed if they are just another generic `esp-radio` hook A/B with
no direct migration consequence.

## Current Priority

Current priority is:

1. finish the true `backend_legacy_port` runtime/backend path
2. validate packet delivery on that path
3. stop spending cycles on standalone hook churn unless it unblocks the port
