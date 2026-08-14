# Source Tree Architecture Follow-ups

- Status: Deferred — no follow-up has an assigned owner or accepted implementation scope
- Last-reviewed: 2026-08-14
- Owner: Unassigned; current code owners are recorded per item
- Origin: [Completed source-tree architecture cleanup](../archive/refactors/source-tree-architecture-cleanup.md)

## Scope

This inventory preserves the six independently scoped contract, API, and ownership questions left by
the completed structural cleanup. It does not sequence them, assign implementation owners, or commit
the project to making any of the changes. Each item needs separate triage, a proven contract, and
owner-specific validation before implementation.

ADR-0012 already decided the package-boundary question: retain `packages/sdcard` provisionally and
reassess it if a future consumer appears. That accepted decision remains in
[ADR-0012](../architecture/0012-sdcard-package-boundary.md), not in this unresolved inventory.

## F1 — Network operational truth

- Preserved follow-up: move Wi-Fi/link/IP/listener operational truth out of observability.
- Current owner: `firmware::net` owns the network runtime, while `firmware::observability` currently
  stores and exposes link, IPv4, and upload-listener state. No follow-up owner is assigned.
- Triage trigger: a proposed change to the ownership or API of Wi-Fi/link/IP/listener status. Any
  implementation also falls under the live Wi-Fi regression gate.
- Status evidence: `src/firmware/observability/recorders/wifi.rs` and
  `src/firmware/observability/recorders/upload_net.rs` still mutate the operational state consumed by
  `src/firmware/net/`; the ownership change has not been made.

## F2 — App-state/scheduling contract

- Preserved follow-up: break the `app_state -> scheduling` dependency with a proven synchronous
  contract.
- Current owner: `firmware::app_state` publishes the snapshot and directly invokes
  `firmware::scheduling`; no follow-up owner is assigned.
- Triage trigger: a proposed change to app-state publication or scheduling-profile application. The
  completed plan records no independent delivery trigger.
- Status evidence: `publish_app_state_snapshot` in `src/firmware/app_state/snapshot.rs` still calls
  `crate::firmware::scheduling::apply_snapshot(snapshot)` synchronously.

## F3 — Remaining global channels and mixed types/constants

- Preserved follow-up: finish domain ownership for the remaining global channels and mixed
  types/constants.
- Current owner: the residual buckets remain `firmware::config::channels` and `firmware::types`; no
  audit owner is assigned.
- Triage trigger: a domain change touches a residual entry whose owner becomes unambiguous, or a
  separately reviewed ownership audit is proposed. The completed cleanup explicitly limited itself to
  entries made unambiguous by its structural slices.
- Status evidence: `src/firmware/config/channels.rs` still contains cross-domain app, storage, serial,
  diagnostics, network, UI, and trace channels, while `src/firmware/types/mod.rs` still re-exports
  cross-domain types and constants.

## F4 — Shared flash/PSRAM service boundary

- Preserved follow-up: decide whether shared flash/PSRAM policy needs a new service boundary.
- Current owner: `firmware::flash` owns the shared flash facade and `firmware::psram` owns allocator
  state and placement-aware allocations; no decision owner is assigned.
- Triage trigger: not recorded. Triage must first establish a concrete ownership or policy problem;
  the presence of both modules is not itself an implementation commitment.
- Status evidence: `src/firmware/flash.rs` and `src/firmware/psram/mod.rs` remain separate current
  owners, and no accepted ADR or live plan was found for a combined service boundary.

## F5 — Public crate surface

- Preserved follow-up: contract the public crate surface to `meditamer::run()`.
- Current owner: the crate root in `src/lib.rs`; no API-contraction owner is assigned.
- Triage trigger: not recorded. Triage must first inventory actual external consumers and compatibility
  constraints before accepting an API change.
- Status evidence: `src/lib.rs` re-exports `system::run`, but also publicly exposes `firmware`,
  `platform`, and the print macros, so the proposed contraction has not occurred.

## F6 — Retained blue-noise asset

- Preserved follow-up: decide whether the retained, unreferenced
  `src/firmware/assets/suminagashi_blue_noise_600.bin` has a future UI owner.
- Current owner: no code owner is recorded; the file is only retained under `src/firmware/assets/`,
  with no future UI owner or deletion owner assigned.
- Triage trigger: a UI feature proposes to consume the asset, or a separately reviewed deletion
  decision is proposed.
- Status evidence: the binary remains at the path above, and the 2026-08-14 first-party source search
  found no code reference to its filename.
