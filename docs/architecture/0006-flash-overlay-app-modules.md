# ADR-0006: Evaluate native flash-overlay app modules

- Status: Proposed — feasibility required
- Date: 2026-08-07
- Amended: 2026-08-09
- References: [ADR-0007](0007-ui-and-application-structure.md),
  [ADR-0008](0008-app-catalogue-and-launcher.md),
  [UI/app rework plan](../development/ui-app-structure-rework-plan.md),
  [DRAM budget](../development/dram-budget.md)

## Context

Meditamer may eventually need native apps that can be added from SD without rebuilding the base
firmware or rebooting when switching. That is not yet an MVP requirement: the product documents name
an ambient view, containment sessions, and offline configuration, all of which can ship in one image.

The current default release image occupies 1,615,504 bytes of the 4,128,768-byte application region.
Its code and read-only data execute through the ESP32 flash MMU, while mutable state lives in internal
RAM or PSRAM. `AppStateStore` proves that small runtime flash writes work with the second core parked;
it does not prove that executable modules can be safely installed, mapped, initialized, and evicted.

The previous proposal combined an open app set, several simultaneously resident apps, fixed
addresses, no position-independent code, and no relocation. Those properties do not compose without
globally preassigned non-overlapping addresses. It also copied only `.text` and `.rodata`, leaving
`.data`, `.bss`, mutable statics, initialization, and app-owned allocation undefined.

## Decision drivers

- The UI/application structure must remain useful if native loading is rejected.
- First-party native code must not weaken boot recovery or corrupt the base on an interrupted install.
- Several resident providers require non-overlapping code, read-only data, and mutable state.
- No provider code or data may be overwritten while any callback, timer, queue entry, or surface
  still references it.
- The interface must be intentionally versioned rather than exposing arbitrary Rust symbols.
- Flash-cache stalls, watchdog exposure, panel timing, SD DMA, wear, and integrity must be measured.

## Decision

Do **not** adopt the flash-overlay loader as a production architecture yet.

Statically linked providers are the baseline through the shell, lifecycle, overlay, and compiled
catalogue phases in ADR-0007 and ADR-0008. Those layers must depend on a provider contract, not on a
loader implementation.

Run one bounded native-loader feasibility spike after the compiled catalogue works. The spike may
use a fixed experimental flash region, but it must not establish production file formats, persistence
schemas, or compatibility promises.

### Required proof

The spike passes only if one build demonstrates all of the following:

1. **Placement and mapping:** two independently built providers occupy non-overlapping instruction,
   read-only-data, and mutable-state ranges and remain callable at the same time. Physical flash
   offsets, virtual addresses, 64 KiB MMU pages, and both-core cache handling are recorded.
2. **Bounded interface:** providers use a small versioned `#[repr(C)]` descriptor and `extern "C"`
   host-function table. No app resolves arbitrary Rust-mangled base symbols.
3. **Complete runtime state:** `.data` initialization, `.bss` zeroing, app-owned model allocation,
   teardown, panic behaviour, and allocator ownership are explicit and tested.
4. **Safe eviction:** a provider is quiesced; its LVGL objects, callbacks, timers, queued modal work,
   navigation frames, and registry entries are synchronously removed; only then may its bytes be
   overwritten. A provider id plus generation rejects stale references.
5. **Transactional install:** length, compatibility, and content integrity are verified before
   commit. Power loss during erase, write, verification, or metadata commit leaves the base bootable
   and the incomplete provider uncallable.
6. **Operational safety:** measured install time and cache-disabled windows do not violate panel,
   touch, SD, radio, watchdog, or multicore constraints. A realistic install/eviction workload
   supports the flash-wear conclusion.
7. **Recovery:** cold boot reconstructs residency from verified metadata, rejects corrupt or
   incompatible images, and reaches a base-resident fallback without executing provider code.

### Decision gate

- **Accept:** write a new ADR that freezes the proven layout, ABI, install transaction, integrity
  policy, capacity model, and operational limits.
- **Reject:** keep statically linked native apps and use SD only for declarative content and assets.
- **Inconclusive:** retain this ADR as Proposed and do not begin external-app installation work.

## Consequences

### Positive

- The UI rework can proceed without betting the product on an unproven loader.
- Loader risk is concentrated in a disposable spike with falsifiable gates.
- A future loader has a narrow, auditable boundary and explicit recovery semantics.

### Negative

- Adding native apps requires a firmware rebuild until the spike passes and a successor ADR is
  accepted.
- The spike must invest in linker, MMU, flash, ABI, and power-failure tooling before producing user
  functionality.

## Alternatives considered

- **Compile apps into one image:** current baseline; simplest and safest for the MVP.
- **Declarative SD apps/content:** suitable when layouts and behaviour fit a bounded host schema.
- **Single rebootable app slot:** larger code allowance and simpler isolation, but switching reboots.
- **Interpreter or VM:** relocatable and isolatable, but adds flash, runtime, and host-ABI costs.
- **Original fixed-address overlay proposal:** not accepted because simultaneous independent modules,
  mutable state, executable mapping, and eviction safety were not resolved.

## Validation

Acceptance evidence belongs in the
[implementation ledger](../development/ui-app-structure-rework-ledger.md). A successful build or a
single direct call is not sufficient to promote this ADR.
