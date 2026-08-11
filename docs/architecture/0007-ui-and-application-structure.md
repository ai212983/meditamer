# ADR-0007: UI shell and application structure

- Status: Proposed
- Date: 2026-08-07
- Amended: 2026-08-10
- References: [ADR-0006](0006-flash-overlay-app-modules.md),
  [ADR-0008](0008-app-catalogue-and-launcher.md),
  [UI/app rework plan](../plans/ui-app-structure-rework-plan.md),
  [UX guidelines](../product/ux-guidelines.md),
  [DRAM budget](../reference/dram-budget.md)

## Context

The LVGL baseline eagerly creates `home` and `gesture_test`. The sibling modules import each other,
own their roots through file-local atomics, and call `lv_screen_load` directly. Adding a screen
therefore changes its neighbours, retains every created tree in the LVGL pool, and leaves no owner
for current-screen, navigation, composition, or teardown policy.

The display task already owns LVGL, dirty-region collection, panel refresh, and the panel-power
lease. `app_state::Phase` owns device lifecycle and diagnostics exclusivity; it is not UI navigation.
These ownership boundaries remain in place.

The structure must support an ambient resting surface, a launcher, app-local depth navigation,
system surfaces, and overlays without depending on whether providers are statically linked or loaded
later.

## Decision drivers

- Exactly one task may call LVGL.
- Screens must not import or address sibling screens.
- LVGL memory use must follow the live surface set rather than everything ever visited.
- Global navigation, modal dismissal, and protected input cannot be captured by an app.
- Refresh mode remains a device policy informed by actual dirty output and surface intent.
- Provider teardown must be complete before provider code or state can disappear.
- Capacity and failure paths must be deterministic in `no_std` operation.

## Decision

Introduce a base-resident `ui::shell` between the display task and UI providers. Providers may be
compiled into the image or supplied by a future loader; the shell contract is the same.

### Vocabulary and ownership

| Term | Meaning |
| --- | --- |
| Provider | Owner of surface definitions and model state; identified by id and generation. |
| Surface definition | Immutable id, role, capabilities, and lifecycle callbacks. |
| Surface instance | One live model plus LVGL root and provider ownership token. |
| Screen | Full-panel surface; exactly one active. |
| Overlay | Surface above the screen with an independent lifetime. |
| Shell | Registry, navigator, compositor, global input, and refresh-intent coordinator. |

The display task owns the shell and LVGL. The shell owns active instances, navigation frames, overlay
ordering, focus, and provider teardown. Providers own plain model data and local widget behaviour.
The refresh module retains panel transaction and power-lease policy.

### Module boundary

```text
src/firmware/ui/
  shell/       registry.rs navigator.rs compositor.rs input.rs intent.rs refresh_hint.rs
  screen/      launcher.rs settings.rs ambient_fallback.rs
  overlay/     confirm.rs cue.rs
  widget/      carousel.rs button.rs
  theme/       type_scale.rs ink.rs
  lvgl/        backend.rs io.rs dither.rs
```

`screen` and `overlay` contain base-resident system surfaces. Product providers register through the
shell and are not hard-coded into navigator branches.

### Provider and instance contract

- Every registered definition carries `ProviderId`, provider generation, `SurfaceId`, role, and
  capabilities. Duplicate ids and capacity exhaustion are explicit registration failures.
- `enter` receives a shell-owned context and returns either a complete instance or an error. Failed
  entry leaves the previous composition active.
- Instance model state is separate from its LVGL root. The model may survive ordinary back-stack
  navigation only when its owner and capacity policy allow it.
- Overlay instances retain both the surface-definition owner and the exact provider generation that
  requested them. Provider teardown matches either edge; a provider-requested base confirmation
  cannot outlive its requester.
- `leave` synchronously removes the LVGL tree and all provider callbacks, timers, user data, and
  queued work. Asynchronous LVGL deletion is forbidden at an eviction boundary.
- Provider removal is one transaction with two explicit commits: detach quiesces the owner and
  atomically purges navigation, composition, and queued references while retaining its registry
  definitions; finalization unregisters only after synchronous runtime and callback-route audits find
  no exact owner token. The shell rejects other mutations between detach and finalization. Provider
  code and state remain pinned if cleanup is retryable and permanently unreleasable after an audit
  failure.

The initial implementation may use Rust-native callbacks because all providers are compiled
together. Any external native provider must satisfy ADR-0006's narrower versioned C ABI.

### Navigation

The shell maintains one depth stack:

```text
ambient root -> launcher -> app root -> app child -> ...
```

- `Back` pops one frame; at app root it returns to the launcher.
- `Home` unwinds to the ambient root.
- Launching another app replaces the app portion of the stack; there is no peer-routing graph.
- Settings and diagnostics are catalogue entries supplied by the base. The device phase gate controls
  whether diagnostics is eligible.
- Surfaces emit `NavIntent`; they never import another surface module or call navigation primitives.

The ambient role has one durable user binding and a base fallback. Binding policy comes from
ADR-0008; it is not part of transient navigation state.

### Composition and input

Composition has three ordered bands: active screen, provider overlays, and base system overlays.
Base confirms and critical cues always rank above provider content.

Provider overlays declare two independent properties:

- input: `Passive`, `Interactive`, or `Modal`;
- lifetime: `Transient` or `Sticky`.

Passive pointer passthrough, bounded interactive hit capture, and modal focus capture must be proven
against LVGL before the generic overlay API is accepted. Interactive overlays capture only their
widget bounds; at most one modal is active. Queued requests retain owner tokens and are purged with
their provider.

Modal scheduling is FIFO within each band, with base-system requests ahead of provider requests;
numeric overlay rank does not change modal scheduling. A base modal preempts and cancels an active
provider modal, and the preempted instance token is never reused. Provider requests cannot exhaust
capacity needed by a protected base confirmation: when necessary, the newest queued provider modal
is canceled; admission fails only when all queued requests are already base-owned.

The shell interprets global gestures, hardware actions, Back, Home, protected confirmation, and
modal dismissal. Providers retain local widget input and emit intents upward.

### Rendering and timing

Instances report refresh intent such as `Micro`, `Content`, or `Boundary`; LVGL still supplies the
actual dirty area. The shell merges live intents, but only the display refresh module chooses partial,
full, fallback, cleanup, and panel-power behaviour.

Explicit user refresh actions also travel upward as source-owned intents. The display task may reject
them under device policy, and only its refresh module executes an accepted panel transaction.

The 8 ms service period is a scheduling target, not a promise across a panel transaction. Validation
records shell/LVGL transition time, longest timer-service gap, touch-to-render latency,
touch-to-panel latency, dirty area, and selected refresh mode separately.

### State lifetime

| State | Lifetime and owner |
| --- | --- |
| Active navigation stack | Volatile; optionally retained in RTC for a short resume window. |
| Ambient binding, pins, enablement, startup config | Durable settings with checksummed recovery. |
| Device lifecycle and diagnostics phase | Existing `AppStateSnapshot`. |
| Provider residency metadata | Defined only if ADR-0006 is accepted. |

RTC retention is deferred until deep sleep exists and explicitly preserves the selected RTC memory.
Invalid or stale retained state always falls back to the ambient root.

### Bounded operation

Registry, stack, overlay, modal queue, and retained-model capacities are compile-time constants
recorded in the DRAM budget. Every overflow has a visible error/fallback and a host test; no capacity
failure may silently drop navigation or leave a partially entered surface.

## Consequences

### Positive

- Screen dependencies become one-way through intents and shell state.
- LVGL allocation tracks active instances and has an explicit reclamation boundary.
- Static providers deliver MVP functionality while preserving a future loader seam.
- Provider identity and generation make stale-reference rejection testable.

### Negative

- Navigation, overlay, and provider lifecycles become explicit state machines with fixed capacities.
- Existing screens must migrate together at the ownership cutover; direct sibling navigation cannot
  remain as a fallback owner.
- Sticky overlays add ghosting, focus, and working-set pressure that require explicit budgets.

## Alternatives considered

- **Named sibling routes:** rejected because every new screen edits existing screens.
- **LVGL screen loading as the architecture:** retained as a rendering primitive, rejected as the
  owner of navigation and lifecycle.
- **Loader-specific shell:** rejected because it blocks safe static-first delivery.
- **Trait objects across an external boundary:** rejected; external modules require a bounded C ABI.

## Validation

Implementation order and gates are defined in the
[plan](../plans/ui-app-structure-rework-plan.md); run-specific evidence is recorded in the
[ledger](../plans/ui-app-structure-rework-ledger.md).
