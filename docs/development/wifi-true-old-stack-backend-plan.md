# Wi-Fi True Old-Stack Backend Plan

## Goal

Build a truly parallel Wi-Fi backend for `backend_legacy_port` that uses the
old working stack semantics directly, centered on:

- `esp-wifi 0.15.1`
- `esp-wifi-sys 0.7.1`

This plan replaces the failed strategy of adapting the current
`esp-radio 0.17.0` internal/blob-facing path to old behavior.

## Why This Plan Exists

The previous two strategies are now closed:

1. Rust-side old-stack import on top of the current backend did not restore
   discovery.
2. The first direct old-internal compatibility cut against the current blob
   regressed init before `esp_wifi_init_internal(...)` completed.

That means the remaining mismatch is not credibly fixable through more
incremental adaptation of the current blob-facing layer. If we still want
stable on-device Wi-Fi discovery, we need a backend that is much closer to the
old working stack as a whole.

## Current Boundary

The last validated active backend result is:

- `backend_legacy_port` is selected
- runtime/bootstrap completes
- direct old-stack Rust import does not restore discovery
- direct blob-compat install contract hangs at:
  - `legacy_port_wifi_init stage=esp_wifi_init_internal.before`

Interpretation:

- runtime/bootstrap work is no longer the priority
- adapter/table/facade work is no longer the priority
- current blob generation is not a stable target for further extraction

## Non-Goals

Do not do any of the following in this plan:

- more runtime/bootstrap refactors
- more A/B flags or diagnostics as a strategy
- more partial old-behavior emulation on the current blob generation
- mixing two active Wi-Fi ownership models in one path without an explicit
  single-owner contract

## Implementation Shape

Build a parallel backend path with these properties:

- new old-stack module/crate boundary
- old stack owns:
  - global Wi-Fi install tables
  - init path
  - start/stop/scan path
  - RX callback registration and RX admission path
  - old internal expectations adjacent to the old blob contract
- current app/runtime code remains the caller
- `backend_legacy_port` becomes a selector between:
  - current backend path
  - true old-stack backend path

The preferred architecture is:

1. isolate old-stack code in a dedicated vendored subtree or crate
2. expose a minimal backend API to the firmware
3. keep the current stack available only as a control path

## Key Technical Risks

### Symbol and Global Conflicts

The old stack and current stack may export overlapping globals or expect
exclusive ownership of:

- Wi-Fi OSI tables
- coex tables
- static global config
- callback registries
- NVS/PHY/Wi-Fi global state

Mitigation:

- do not link both as active backends simultaneously
- isolate old-stack symbols behind a dedicated module/crate boundary
- keep one active backend owner at runtime

### Crate Graph Conflicts

The current repo already vendors `esp-radio 0.17.0`. Pulling in the old stack
naively may cause:

- duplicate symbol definitions
- incompatible bindgen layouts
- conflicting feature assumptions

Mitigation:

- start with a dedicated vendored old-stack substrate instead of mixing it
  directly into the active current `wifi/` tree
- adapt at the backend boundary, not throughout the repo

### Runtime Ownership Conflicts

The old stack may assume ownership of scheduler/task/global state differently.

Mitigation:

- keep current runtime/bootstrap unless the old stack proves it requires
  tighter ownership
- if needed, migrate only the minimum runtime pieces required by the old stack,
  but only after backend isolation is in place

## Success Criteria

The true old-stack backend is only considered successful if canonical
full-flash validation shows one or more of:

- pre-scan promisc becomes non-zero
- `wifi_rx_cb_count` becomes non-zero
- `scan_done_eventpost` becomes non-zero
- direct explicit scan returns APs
- wrapped scan returns APs

Secondary success criteria:

- init completes reliably
- `start=ok`
- repeated validation runs produce the same discovery behavior

## Validation Rules

Use only:

- canonical full-flash `hostctl flash-capture`
- `wifi-debug-slim-app`
- `backend_legacy_port` diagnostics

Record after each runtime-affecting phase:

- backend name
- `runtime_init result`
- `legacy_port_wifi_init stage=done`
- `start=ok`
- pre-scan promisc totals
- direct null scan result
- direct explicit scan AP count
- wrapped scan result
- `wifi_mac_isr_count`
- `wifi_rx_cb_count`
- `scan_done_eventpost`
- queue/semaphore/thread-semaphore counters

## Phases

- [x] Phase 1: isolate a dedicated old-stack substrate
- [x] Phase 2: define the backend API boundary
- [~] Phase 3: wire old init/install ownership
- [~] Phase 4: wire old control/scan ownership
- [~] Phase 5: wire old RX delivery ownership
- [ ] Phase 6: cut `backend_legacy_port` over to the true old stack
- [ ] Phase 7: validate discovery behavior
- [ ] Phase 8: decide continue vs stop

## Phase 1: Isolate A Dedicated Old-Stack Substrate

### Goal

Create a clean implementation boundary for the old working stack instead of
continuing to patch the current vendored `wifi/` tree.

### Steps

- [x] Step 1.1 choose the concrete layout:
  - vendored subtree under `vendor/`
  - or dedicated internal crate
- [x] Step 1.2 import the minimum old stack sources needed for:
  - install/init
  - control/scan
  - RX delivery
- [x] Step 1.3 ensure the old substrate compiles in isolation from the active
      current Wi-Fi path

### Deliverable

A new source boundary that can host the old stack without continued mutation of
the current `esp-radio 0.17.0` Wi-Fi internals.

Notes:

- commit:
- validation:
  `CARGO_FEATURES=wifi-debug-slim-app scripts/build/build.sh debug`
- outcome:
  - chose a dedicated vendored subtree under:
    `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/vendor/esp-radio-0.17.0/src/wifi/true_old_stack/`
  - added isolated old-stack modules for:
    - `install.rs`
    - `init.rs`
    - `control.rs`
    - `rx.rs`
    - `mod.rs`
  - kept this phase compile-only by bridging through the existing legacy path
    rather than cutting over behavior
  - the old-stack substrate now compiles without changing the active backend

## Phase 2: Define The Backend API Boundary

### Goal

Keep the firmware-facing contract small so the rest of the app does not care
which Wi-Fi backend is active.

### Required API Surface

The true old-stack backend should expose only what `backend_legacy_port`
actually needs:

- `wifi_new`
- `start`
- `stop`
- `scan_with_config`
- RX callback registration
- RX token / consume path if still required by the caller

### Steps

- [x] Step 2.1 define a minimal internal backend trait or module contract
- [x] Step 2.2 adapt old-stack types only at this boundary
- [x] Step 2.3 keep current backend API stable to firmware callers

Notes:

- commit:
- validation:
  `CARGO_FEATURES=wifi-debug-slim-app scripts/build/build.sh debug`
- outcome:
  - established the minimal true-old-stack backend surface in
    `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/vendor/esp-radio-0.17.0/src/wifi/true_old_stack/mod.rs`
  - exposed compile-time backend entrypoints from
    `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/vendor/esp-radio-0.17.0/src/wifi/mod.rs`
    for:
    - `wifi_new`
    - `start`
    - `stop`
    - `scan_with_config`
  - kept firmware-facing callers unchanged and did not switch the active
    backend yet

## Phase 3: Wire Old Init/Install Ownership

### Goal

Move active ownership of old install/global/init behavior into the true
old-stack backend.

### Steps

- [ ] Step 3.1 port old OSI/coex/global table ownership
- [ ] Step 3.2 port old `wifi_new` / `wifi_init`
- [ ] Step 3.3 port old RX callback registration during init
- [ ] Step 3.4 keep runtime/bootstrap unchanged unless the old stack proves
      incompatible without additional migration

### Validation

Build first. Then canonical validation to confirm:

- old backend selected
- init completes or fails at a new, precise boundary

Notes:

- commit:
- validation:
  `CARGO_FEATURES=wifi-debug-slim-app scripts/build/build.sh debug`
- outcome:
  - `true_old_stack/install.rs` now contains a literal copy of the current
    old-stack install/global ownership instead of a bridge stub
  - `true_old_stack/init.rs` now contains a literal copy of the current
    old-stack init path and depends on the isolated subtree install module
  - this phase is still in progress because the active backend has not been
    cut over yet

## Phase 4: Wire Old Control/Scan Ownership

### Goal

Make the true old-stack backend own the start/stop/scan path completely.

### Steps

- [ ] Step 4.1 port old `start`
- [ ] Step 4.2 port old `stop`
- [ ] Step 4.3 port old blocking scan flow
- [ ] Step 4.4 port result retrieval / clear behavior

### Validation

Canonical full-flash boot-scan only.

Record:

- direct null scan result
- direct explicit scan AP count
- wrapped scan result

Notes:

- commit:
- validation:
  `CARGO_FEATURES=wifi-debug-slim-app scripts/build/build.sh debug`
- outcome:
  - `true_old_stack/control.rs` now contains the literal current old-stack
    control/scan implementation
  - active backend ownership has not been switched yet, so runtime validation
    is deferred to the cutover phase

## Phase 5: Wire Old RX Delivery Ownership

### Goal

Make the true old-stack backend own RX delivery and admission-adjacent
behavior.

### Steps

- [ ] Step 5.1 port old `recv_cb_sta`
- [ ] Step 5.2 port old `recv_cb_ap`
- [ ] Step 5.3 port old RX queue / packet buffer ownership
- [ ] Step 5.4 port old RX token / consume path if used by the caller

### Validation

Canonical full-flash boot-scan only.

Record:

- pre-scan promisc totals
- `wifi_mac_isr_count`
- `wifi_rx_cb_count`
- `scan_done_eventpost`
- raw scan state

Notes:

- commit:
- validation:
  `CARGO_FEATURES=wifi-debug-slim-app scripts/build/build.sh debug`
- outcome:
  - `true_old_stack/rx.rs` now contains the literal current old-stack RX
    delivery implementation
  - the isolated subtree now hosts install/init/control/RX as one coherent
    compile-clean unit
  - active runtime validation is deferred until the explicit backend cutover

## Phase 6: Cut `backend_legacy_port` Over To The True Old Stack

### Goal

Make the firmware use the true old-stack backend as the active implementation.

### Steps

- [ ] Step 6.1 switch `backend_legacy_port` init/control/RX entrypoints to the
      isolated old backend
- [ ] Step 6.2 keep old shim-based adaptation code only as compile support
- [ ] Step 6.3 do not delete current fallback paths in the same step

### Validation

Canonical full-flash boot-scan only.

## Phase 7: Validate Discovery Behavior

### Goal

Determine whether the true old-stack backend actually restores discovery.

### Steps

- [ ] Step 7.1 run canonical validation
- [ ] Step 7.2 compare against the current control baseline
- [ ] Step 7.3 classify whether discovery metrics moved meaningfully

### Success Threshold

At least one discovery metric must move off zero.

## Phase 8: Decide Continue vs Stop

### If Discovery Improves

- [ ] Step 8.1 stabilize repeated runs
- [ ] Step 8.2 remove dead shim paths in follow-up work
- [ ] Step 8.3 begin connect/upload validation only after discovery is stable

### If Discovery Does Not Improve

- [ ] Step 8.4 stop source-level backend work on this branch
- [ ] Step 8.5 record that even the true old-stack backend does not cross the
      remaining boundary
- [ ] Step 8.6 reassess product/backend strategy outside the current approach

## First Recommended Step

Start Phase 6 by cutting `backend_legacy_port` over to the isolated
`true_old_stack/` subtree for init/control/RX while keeping runtime/bootstrap
unchanged.

## Stop Conditions

Stop immediately if:

- the implementation starts mutating runtime/bootstrap again
- the new work becomes another compatibility-extraction branch on the current
  blob generation
- canonical validation does not use `backend_legacy_port`
- repeated full-flash validation cannot be executed
