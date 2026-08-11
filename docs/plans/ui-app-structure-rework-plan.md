# UI and App Structure Rework Plan

- Status: Done
- Last-reviewed: 2026-08-11
- Started: 2026-08-09
- Completed: 2026-08-11
- Decisions: [ADR-0006](../architecture/0006-flash-overlay-app-modules.md),
  [ADR-0007](../architecture/0007-ui-and-application-structure.md),
  [ADR-0008](../architecture/0008-app-catalogue-and-launcher.md),
  [ADR-0009](../architecture/0009-ab-firmware-update-foundation.md),
  [ADR-0010](../architecture/0010-durable-ui-settings.md)
- Evidence: [Implementation ledger](ui-app-structure-rework-ledger.md)

## Objective

Replace sibling-owned screens and eager LVGL trees with a bounded, base-resident shell that supports
static applications, overlays, a catalogue, and durable user choices. Preserve the existing display
task, touch pipeline, dirty-region conversion, panel refresh policy, and device lifecycle ownership.

Native app installation is an optional research branch. The product path must remain complete if the
loader is rejected.

Safe full-firmware replacement is a separate system concern. A bounded A/B feasibility and
foundation gate follows the compiled catalogue so the launcher can expose a base-owned update surface,
but precedes durable UI settings and any native-loader flash allocation.

## Invariants

1. Only the display task calls LVGL.
2. Surfaces emit intents; only the shell changes navigation or composition.
3. One active screen exists; overlays have explicit input, lifetime, rank, and owner tokens.
4. No direct `home`/`gesture_test` sibling dependency survives the ownership cutover.
5. Entry failure preserves the previous valid composition.
6. Provider teardown removes every live and queued reference before code or state can disappear.
7. Fixed capacities are measured, tested, and recorded in the DRAM budget.
8. Actual dirty output and refresh policy remain separate from a surface's refresh intent.
9. Durable settings, volatile navigation, device lifecycle, and provider residency remain separate.
10. A phase advances only after its acceptance evidence is entered in the ledger.
11. Firmware staging, validation, activation, and boot confirmation are owned by a base system service;
    UI providers may request an update and present status but never own flash or boot metadata.

Explicit user direction may prepare non-hardware work from a later phase while an earlier physical
gate is unavailable. That work remains provisional: record the deviation, do not mark either phase
complete, and do not use provisional implementation as evidence that a prerequisite passed.

## Scope boundaries

In scope:

- shell contracts and pure state machines;
- static provider registration;
- lazy screen lifecycle;
- base and provider overlays;
- compiled catalogue, launcher, and filtered views;
- a bounded A/B firmware-update feasibility and foundation gate;
- durable UI settings and optional short-window navigation retention;
- a bounded native-loader feasibility spike and decision gate.

Out of scope until explicitly promoted:

- arbitrary third-party code;
- production native-module format or compatibility promise;
- install-on-launch;
- SD catalogue caching;
- production network update transport before the A/B foundation passes;
- background services owned by UI providers;
- visual redesign unrelated to the ownership migration.

## Phase 0: Decision and execution baseline

Amend ADR-0006 through ADR-0008, supersede the obsolete hybrid-renderer TODO, establish this plan,
and create the ledger.

Acceptance:

- ADRs separate durable decisions from run-specific implementation detail;
- ADR-0007 and ADR-0008 do not depend on a successful native loader;
- ADR-0006 contains falsifiable proof and stop gates;
- Markdown links, LOC policy, formatting, and diff checks pass;
- the ledger identifies Phase 1 as the only next implementation phase.

## Phase 1: Shell contract and pure state model

Define ids, owner generations, roles, capabilities, intents, refresh hints, fixed capacities,
registration errors, navigation frames, a bounded intent queue, and provider-removal semantics without
creating LVGL objects.

Acceptance:

- host tests cover duplicate ids, capacity exhaustion, Back, Home, app replacement, failed entry
  rollback, stale owner generations, and provider-reference purging;
- no product surface id appears in navigator branching;
- DRAM estimates for provider authority, registry, stack, overlays, modal and intent queues, and the
  retained-model policy are in the ledger;
- no production screen ownership changes yet.

Stop if the model needs an unbounded collection, hidden allocator dependency, or a second navigation
owner.

## Phase 2: Static navigation vertical slice

Place the shell in the display-task-owned LVGL backend. Register `home` as the fallback ambient
provider and `gesture_test` as a base diagnostics entry. Route their navigation through intents.

Acceptance:

- both screens remain behaviourally reachable through one shell-owned stack;
- direct sibling imports and direct surface-owned `lv_screen_load` calls are absent;
- the previous composition survives a failed destination entry;
- existing touch, multitouch, dirty-region, refresh, and panel-lease host coverage passes;
- target hardware confirms navigation and refresh mode with an identified firmware artifact.

The phase is not complete if old direct navigation remains as a fallback owner.

## Phase 3: Lazy lifecycle and resource proof

Create only the active screen tree, synchronously destroy the prior tree after a successful
transition, and make model retention an explicit bounded policy.

Acceptance:

- repeated ambient/diagnostics cycles return LVGL pool use to the expected baseline;
- deletion removes callbacks, timers, user data, and file-local root pointers;
- injected entry and leave failures recover to a valid base surface;
- LVGL pool high-water, internal DRAM, stack, PSRAM, transition time, and timer-service gaps are
  recorded for debug and release builds;
- hardware shows no unexpected full refresh, panic, watchdog, or touch regression.

Stop if synchronous teardown cannot prove that the old tree is unreachable.

## Phase 4: Composition and input arbitration

Add the compositor using base-owned examples first: a confirm modal, a passive cue, and one sticky
interactive overlay. After base behaviour is proven, add one statically linked provider fixture to
exercise exact detach, runtime cleanup, callback audit, and final unregister before any generic
provider API.

Acceptance:

- passive overlays demonstrably pass pointer input to the screen beneath;
- interactive overlays capture only their widget bounds and emit source-owned actions;
- modal focus, dismissal, queueing, and base rank are deterministic;
- protected base modals preempt provider modals and cannot be starved by provider queue capacity;
- navigation drops transient overlays and preserves sticky overlays;
- provider removal purges definition-owned and requester-owned live/queued overlays by generation,
  then defers unregister until runtime and callback audits pass;
- dirty-area merging and refresh intent do not bypass refresh policy;
- a sticky-overlay navigation cycle stays within the recorded resource and ghosting budgets.

## Phase 5: Compiled catalogue and launcher

Implement ADR-0008 with base and statically linked entries only. Add launcher, ambient-picker, and
overlay-settings presenters over the same catalogue.

Acceptance:

- capability filters produce the expected views under one stable entry id;
- source, residency, compatibility, and health axes are tested independently;
- ordering is pins, stable default rank, then id and does not change after launch;
- empty/exhausted/faulted cases retain base entries and an ambient fallback;
- catalogue and presenter capacities are in the DRAM ledger;
- no SD scan or native install dependency exists.

## Phase 5A: A/B firmware-update feasibility and foundation decision

Run after Phase 5 and before choosing the Phase 6 storage layout. Treat A/B as boot and recovery
infrastructure, not as a launcher implementation. First prove host- or SD-staged replacement; network
transport and polished update UX are not prerequisites for the gate.

Acceptance:

- an exact 4 MiB partition budget fits two copies of the measured release application with explicit
  margins, alignment, OTA metadata, NVS/PHY needs, and a non-overlapping app-state partition;
- the exact shipped bootloader binary and configuration are pinned and prove OTA selection,
  pending-verify handling, automatic rollback, and serial recovery without relying on an assumed
  `espflash` default;
- `AppStateStore` moves out of the final flash sector with an interruption-safe, idempotent migration
  from the single-image layout;
- full-flash and app-only host workflows resolve partition labels and offsets from the accepted layout;
  no production fallback retains a hard-coded `0x10000` application target;
- a base-owned update service erases and writes only the inactive slot in bounded chunks, verifies
  image structure, exact length, compatibility, and content authenticity before activation, and
  exposes status without transferring flash ownership to LVGL;
- first boot confirms the candidate only after a bounded software-health gate; serial readiness is not
  treated as physical panel or touch proof;
- power interruption during erase, write, verification, metadata activation, candidate boot, and
  confirmation leaves the previous slot bootable or causes a demonstrated automatic rollback;
- exact artifacts, slot identities, boot reasons, update timing, cache-disabled windows, watchdog and
  multicore behaviour, flash wear assumptions, and remaining image headroom are recorded in the ledger.

Stop if two current images do not retain an explicit growth margin, the bootloader cannot prove
automatic rollback, state migration can overwrite either slot, or recovery depends on retrying an
ambiguous transaction.

At the gate:

- accepted proof creates an ADR that freezes the bootloader, partition map, update transaction,
  authenticity policy, health-confirmation boundary, recovery path, and capacity floor before
  production transport or UX work;
- rejected proof retains the single-image flash path and records the stable storage layout that Phase 6
  may use;
- inconclusive proof stops without adding a launcher update entry or network firmware transport.

## Phase 5B: Serial recovery and A/B transport throughput

Run after Phase 5A has fixed the flash map and recovery authority. Improve developer full-flash and
signed serial-update throughput without weakening exact-artifact capture, inactive-slot isolation,
authenticity, full read-back, activation, rollback, or candidate-confirmation gates.

Acceptance:

- exact full flash uses the proven stub-assisted rate as its primary attempt and retries the same
  bootloader, partition table, OTA data, and application transaction through a conservative ROM-only
  path; automatic recovery never changes an A/B full flash into an app-only write;
- the application update protocol negotiates capabilities before changing transport, retains the
  Phase 5A hex protocol for older firmware, and restores the boot UART rate before line commands;
- every binary frame, including header and CRC, fits the 128-byte UART FIFO; accepted payloads are
  coalesced into internal-RAM writes no larger than the proven 256-byte flash-call ceiling;
- missing acknowledgements and CRC failures may retry only the immediately previous identical frame;
  explicit flash/protocol errors and ambiguous activation acknowledgements still stop;
- baud and frame-size trials vary one transport property at a time and retain failed-run evidence;
- a complete signed inactive-slot update proves the staged digest, full read-back, activation,
  pending-verify boot, software-health confirmation, final slot identity, and end-to-end timing.

This phase does not add network firmware transport, compression, update UX, or a launcher entry.

## Phase 6: Durable UI settings and optional resume

Run only after Phase 5A records an accepted or rejected stable flash-layout decision. Define versioned,
checksummed, recoverable storage for ambient binding, pins, enablement, and startup composition. Add
short-window navigation retention only after deep sleep explicitly retains the chosen RTC memory.

Acceptance:

- interrupted writes, corrupt versions, unknown ids, unavailable providers, and write-rate limits
  have deterministic fallbacks;
- boot never installs a provider and always reaches the base ambient surface;
- durable UI settings do not overlap app state, OTA metadata, either firmware slot, or any explicitly
  reserved recovery region;
- volatile navigation does not enter `AppStateSnapshot` or durable UI settings;
- invalid or stale RTC state falls back without a boot loop;
- cold-boot and deep-sleep evidence distinguish serial readiness from physical panel correctness.

Completed 2026-08-11. E-0016 and E-0017 cover the implementation, identified-artifact boot, and
host recovery gates; E-0018 covers deferred-save persistence across physical power cycles plus panel
legibility and touch operation. Optional RTC navigation resume remains unimplemented because retained
RTC memory is not configured, so no deep-sleep-resume claim is made.

## Phase 7: Native-loader feasibility spike and decision

Run only after Phases 1 through 5 are stable and Phase 5A has fixed the flash-layout direction. If A/B
is accepted, first recalculate whether any non-overlapping native-module region remains without
reducing either firmware slot below its capacity floor. Absence of such a region parks or rejects the
native-loader path without implementation. Otherwise implement the smallest two-provider experiment
that can falsify ADR-0006's placement, ABI, runtime-state, eviction, transaction, operational, and
recovery requirements.

Acceptance is exactly ADR-0006's required proof. Partial success is not promotion evidence.

At the gate:

- accepted proof creates a successor ADR before production implementation;
- rejected proof records the failure and closes the native-loader path;
- inconclusive proof stops without beginning external installation or compatibility work.

## Phase 8: External catalogue and installation, conditional

This phase does not start unless Phase 7 produces an accepted successor ADR. Define bounded manifests,
content integrity, discovery, measured caching, transactional installation, and removal UX.

Acceptance:

- malformed and conflicting manifests cannot hide or corrupt base entries;
- catalogue performance is measured from card power-off through first render;
- cache correctness never depends on FAT timestamps alone;
- installation is acknowledged, verified, power-fail-safe, and never runs during boot;
- removal and eviction satisfy the provider teardown transaction;
- supported hardware passes repeated install, switch, sleep, boot, corruption, and power-cut matrices.

## Terminal state

The base-resident product path through Phase 6 is complete. Phase 7 is parked because ADR-0009 leaves
no non-overlapping native-module region above the firmware capacity floor, so Phase 8's prerequisite
is unmet and external installation remains out of scope. Reopening either branch requires a successor
capacity decision; it is not remaining work in this plan.

## Validation and evidence policy

- Record exact commit/worktree identity, feature profile, toolchain, ELF or image hash, device, and
  configuration for firmware evidence.
- Run focused host tests first, then canonical source, host, firmware, static, and quality lanes in
  proportion to the phase's risk.
- Use debug and release firmware where timing, memory, linker placement, or cache behaviour matters.
- Treat `RUNTIME_READY` as boot evidence, not proof of display content, touch behaviour, or visual
  quality.
- Record expected unrelated failures separately; do not weaken a gate or add retries to hide them.
- Update the ledger before beginning the next phase.
