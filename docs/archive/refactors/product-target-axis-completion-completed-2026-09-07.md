# Product and Target Axis Completion Plan

- Status: Done — all five phases passed; final closeout recorded 2026-09-07
- Last-reviewed: 2026-09-07
- Decision: [ADR-0023 — Platform, board, product, and target axes](../../architecture/0023-platform-board-product-target-axes.md)
- Ledger: [Product and target axis implementation ledger](product-target-axis-completion-ledger-completed-2026-09-07.md)
- Origin: [Completed source-tree architecture cleanup](source-tree-architecture-cleanup.md)

## Closeout (2026-09-07)

All five phases are Passed. Ledger E-0011 records the final completion review
against E-0002–E-0004, E-0007, E-0009 and E-0010. Product/board/target ownership,
build roots, shared runtime ports and inherited follow-up dispositions are
complete on that evidence. No new full-suite validation of the accumulated
working tree is claimed. Broader feature qualification remains in the active
parity and UI plans.

## Goal

Complete the product axis for Meditamer and Medinote while preserving the platform and board
boundaries established by ADR-0023. This plan owns source placement, composition, and build roots;
feature roadmaps remain in their product plans.

## Accepted topology

```text
platform/                         # reusable services and contracts
boards/
  inkplate-tempera/              # Inkplate Tempera hardware support
  waveshare-rlcd42/              # Waveshare 4.2-inch reflective LCD support
products/
  meditamer/                     # meditation product behavior and presentation
  medinote/                      # note product behavior and presentation
targets/
  meditamer-inkplate/            # Meditamer + Inkplate Tempera firmware
  medinote-waveshare/            # Medinote + Waveshare firmware
```

The repository's shared workspace hosts the platform and product libraries used by host tests. Each
`targets/*` directory is an independent firmware build root with the target-specific Rust toolchain,
Cargo configuration, feature selection, and artifact commands required by its ESP32 or ESP32-S3.

`targets/` is the composition surface. Ownership follows ADR-0023's four axes:

- `platform/` supplies reusable contracts and services.
- `boards/` supplies hardware drivers and board adapters.
- `products/` supplies product state, policy, content, and presentation.
- Each target combines exactly one product with one board and owns chip startup, task wiring, and
  firmware artifact configuration.
- Dependency flow is `targets -> products/boards -> platform`; products and boards are siblings.

## Current state

- Both product libraries, board libraries, and target build roots follow the accepted topology;
  phases 1-3 are complete.
- `platform/netstack` and `platform/runtime` own the extracted network runtime, configuration,
  telemetry, and shared log filtering. Meditamer/Inkplate consumes those services and has passed
  source, host, target-build, and ELF/DRAM validation.
- Phase 4 passed: E-0010 records the September 7 Inkplate Wi-Fi regression gate
  on an identified artifact, completing the extraction acceptance evidence.
- Medinote/Waveshare now requires BLE, Wi-Fi and SD capability parity with Meditamer/Inkplate.
  [Connectivity and storage parity](../../plans/connectivity-storage-parity.md) owns that delivery;
  [shared Wi-Fi services](../wifi/shared-wifi-services-and-waveshare-completed-2026-09-07.md) owns its `platform/netstack`
  adoption, and [shared storage](../hardware/shared-storage-and-waveshare-sd-completed-2026-09-07.md) owns SD bring-up and reuse.
  Their wider target qualification remains separate from Phase 4's completed extraction gate.
- Phase 5 is complete: `products/meditamer/src/firmware`'s visibility is narrowed to exactly what
  `targets/meditamer-inkplate` reaches (82% of the wholesale-`pub` surface Phase 3 produced is now
  crate-private), compiler-verified across every build profile and binary.

## Delivery phases

### 1. Establish build roots and guards

- Add the two product library roots and the two target build roots.
- Give each target explicit build, check, flash, and artifact commands.
- Extend repository checks so both target graphs participate in module reachability, code-size, and
  documentation validation.
- Keep compatibility entry points until each replacement target produces equivalent artifacts.

Exit: Cargo metadata resolves for the shared workspace and both target roots, and the documented
commands exercise all three graphs.

### 2. Separate Medinote from the Waveshare board

- Move Medinote state, content, screen construction, cadence policy, and presentation into
  `products/medinote`.
- Keep panel driving, board sensors, timing primitives, and hardware adapters in
  `boards/waveshare-rlcd42`.
- Wire both libraries in `targets/medinote-waveshare` and preserve the current on-device behavior.
- Add host tests for Medinote state and presentation contracts.

Exit: the Waveshare crate is a board library, the Medinote crate is host-testable, and the target owns
their concrete composition.

### 3. Separate Meditamer from the Inkplate target

- Move meditation state, scheduling policy, screens, catalogue, settings, and product assets into
  `products/meditamer`.
- Keep Inkplate hardware behavior in `boards/inkplate-tempera`.
- Move ESP32 startup, Embassy task wiring, firmware update wiring, and artifact configuration into
  `targets/meditamer-inkplate`.
- Preserve existing binary names and release behavior through the migration.

Exit: Meditamer product behavior is host-testable and the target builds the established Inkplate
firmware artifact.

### 4. Complete shared runtime extraction

- Design: [Phase 4 shared runtime design](product-target-axis-phase-4-shared-runtime-completed-2026-09-07.md).
- Extract the current network runtime into `platform/netstack` around the established host contract.
- Move network-owned channels, types, listener state, and link/IP truth with that owner.
- Supply product and target policy through ports implemented at the composition boundary.
- Compare the module census before and after the move, then run the live Wi-Fi regression gate.
- Deliver Medinote's now-required adoption under the linked capability plans, using the same
  runtime contracts and separate S3 resource/lifecycle qualification.

Exit: Meditamer consumes the shared runtime through explicit contracts, operational state is owned by
the runtime, host/build/ELF checks pass, and the Meditamer Wi-Fi regression gate passes. A second
product consumer is not required for this extraction gate; Medinote adoption is required by the
separate active parity plan.

### 5. Contract surfaces and resolve residual ownership

Start with a consumer census of the broad `meditamer_product::firmware` surface and the compatibility
re-exports retained during phases 3-4. Classify each exposed item as product API, target composition
port, platform API, or private implementation; move or narrow it at its real owner, then remove the
compatibility path after its consumers migrate.

Apply the following disposition during that contraction:

| Prior follow-up | Disposition |
| --- | --- |
| F1 — network operational truth | Delivered with `platform/netstack` in phase 4. |
| F2 — app-state/scheduling contract | Meditamer-owned synchronous contract; revisit when a shared scheduling consumer exists. |
| F3 — global channels and mixed types | Move each entry with its domain owner during phases 2–4. |
| F4 — flash/PSRAM service boundary | Keep separate ownership; introduce a focused contract when a target or second consumer requires one. |
| F5 — public crate surface | Replace the broad root facade with bounded platform, product, board, and target APIs. |
| F6 — blue-noise asset | Move to the Meditamer asset owner when used, or remove it during product extraction. |

Exit: public APIs match active consumers, residual types and channels have domain owners,
`meditamer_product::firmware` is no longer a catch-all public facade, and every removed compatibility
path has a direct replacement at its owning domain.

## Validation

For each phase:

- Run shared-workspace host tests and lints.
- Build release artifacts from both target roots in the documented distrobox environment.
- Run `scripts/ci/check_orphan_modules.py`, `scripts/ci/lint_code_analysis.sh`, and the Markdown checks.
- Exercise the affected target on hardware when startup, task wiring, display, power, storage, or
  networking behavior changes.
- Run [the Wi-Fi regression gate](../../guides/wifi-regression-gate.md) for Wi-Fi, network, or upload
  changes.

## Completion criteria

- Both products have explicit library ownership and host coverage.
- Both boards expose product-neutral hardware APIs.
- Both targets build and pass the relevant hardware smoke checks.
- Dependency flow matches the accepted topology.
- Shared runtime services have explicit product and target ports.
- The six inherited follow-ups are delivered or resolved by the recorded disposition.
- Live documentation names the new build and source entry points.
