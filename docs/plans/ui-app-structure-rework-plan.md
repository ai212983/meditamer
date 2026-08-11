# UI and App Structure Rework Plan

- Status: Active
- Last-reviewed: 2026-08-10
- Started: 2026-08-09
- Decisions: [ADR-0006](../architecture/0006-flash-overlay-app-modules.md),
  [ADR-0007](../architecture/0007-ui-and-application-structure.md),
  [ADR-0008](../architecture/0008-app-catalogue-and-launcher.md)
- Evidence: [Implementation ledger](ui-app-structure-rework-ledger.md)

## Objective

Replace sibling-owned screens and eager LVGL trees with a bounded, base-resident shell that supports
static applications, overlays, a catalogue, and durable user choices. Preserve the existing display
task, touch pipeline, dirty-region conversion, panel refresh policy, and device lifecycle ownership.

Native app installation is an optional research branch. The product path must remain complete if the
loader is rejected.

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
- durable UI settings and optional short-window navigation retention;
- a bounded native-loader feasibility spike and decision gate.

Out of scope until explicitly promoted:

- arbitrary third-party code;
- production native-module format or compatibility promise;
- install-on-launch;
- SD catalogue caching;
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

## Phase 6: Durable UI settings and optional resume

Define versioned, checksummed, recoverable storage for ambient binding, pins, enablement, and startup
composition. Add short-window navigation retention only after deep sleep explicitly retains the chosen
RTC memory.

Acceptance:

- interrupted writes, corrupt versions, unknown ids, unavailable providers, and write-rate limits
  have deterministic fallbacks;
- boot never installs a provider and always reaches the base ambient surface;
- volatile navigation does not enter `AppStateSnapshot` or durable UI settings;
- invalid or stale RTC state falls back without a boot loop;
- cold-boot and deep-sleep evidence distinguish serial readiness from physical panel correctness.

## Phase 7: Native-loader feasibility spike and decision

Run only after Phases 1 through 5 are stable. Implement the smallest two-provider experiment that can
falsify ADR-0006's placement, ABI, runtime-state, eviction, transaction, operational, and recovery
requirements.

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
