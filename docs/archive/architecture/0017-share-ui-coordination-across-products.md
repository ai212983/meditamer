# ADR-0017: Share UI coordination across products

- Status: Superseded by [ADR-0020](../../architecture/0020-ui-coordination-as-built.md)
- Author: Codex/GPT-5
- Date: 2026-08-26
- Amended: 2026-08-28 (Phase 0 design: LVGL access token, widget ownership, cleanup API, and
  failure recovery -- Claude/Sonnet5, see the dated section after "Alternatives considered")
- Amended: 2026-08-28 (input ownership clarification -- Codex/GPT-5, see the final section)
- Amended: 2026-08-29 (implementation-plan link cleanup; decision unchanged)
- Amended: 2026-08-29 (`UiAccessToken` capability tightening -- Claude/Sonnet5, see the
  final section)
- Amended: 2026-08-29 (reconciling the Decision section's contract with what was actually
  built, and the checked-LVGL boundary's completion -- Claude/Sonnet5, see the final section)
- Superseded: 2026-08-29 by [ADR-0020](0020-ui-coordination-as-built.md) -- Claude/Sonnet5.
  The prior amendment already noted "a future ADR should retitle or supersede the ones that
  never shipped rather than this amendment carrying that weight indefinitely"; this project's
  own convention treats a decision reversal as a new ADR, not a further in-place amendment,
  so ADR-0020 is the coordinator's decision record from here on. This document is retained
  for its historical Phase 0 design record (access token, widget ownership, cleanup API) and
  the amendment trail, not as the current contract.
- References: [UI shell and application structure](0007-ui-and-application-structure.md),
  [App catalogue and launcher](0008-app-catalogue-and-launcher.md),
  [Compiled-only UI catalogue](0013-compiled-only-ui-catalogue.md),
  [Two-product platform workspace](0015-two-product-platform-workspace.md),
  [DRAM budget](../reference/dram/dram-budget.md)

## Context

Meditamer and Medinote share `platform/shell`, but they connect it to their visible UI in different
ways. Meditamer has a transactional backend around the shell, LVGL, focus, refresh policy,
and lifecycle recovery. Medinote commits shell navigation and rebuilds LVGL in separate steps.
Failure between those steps can leave the logical shell and visible composition out of sync.

Both products are early and will add screens, overlays, and background subsystems. Their UI state
already has independent parts: navigation, overlays, focus, transition progress, device phase, and
subsystem activity. A base screen may remain active while an overlay temporarily owns focus.

ADR-0007 established the shell, navigation stack, overlays, and single display owner. ADR-0015 moved
the product-neutral shell into the shared platform workspace. This decision defines the shared
coordination layer that connects those mechanisms to each target's renderer and product policy.

## Decision

Add a renderer-independent `UiCoordinator` to `platform/shell`. The coordinator contains the
existing `ShellModel` and adds focus, transition, effect dispatch, and product-policy coordination.
`ShellModel` remains the authority for navigation, overlays, provider generations, and surface
instance identity.

One UI task per target owns the coordinator, its LVGL runtime, focus, visible surface lifetimes, and
refresh intents. Screens, overlays, product code, and background subsystems communicate with that
owner through typed events and results.

The shared interaction contract consists of four types:

- `UiEvent<P>` describes shared navigation, overlay, input, timer, or subsystem activity and can
  carry a typed product or target payload `P`.
- `UiTransaction` identifies the expected shell version, affected surface instances, focus change,
  and merged refresh intent.
- `UiEffect` asks the target adapter to create, activate, pause, resume, destroy, focus, persist, or
  refresh a target-owned resource.
- `UiEffectResult` reports success or a typed failure with the transaction identity.

Each product or target defines its payload type. Its policy translates those payloads into shared
coordinator actions and target effects. This keeps Inkplate- and S3-specific data typed at the
boundary while the shared coordinator stays independent of either board.

The coordinator processes one visible transaction at a time. It prepares the logical change while
the current composition remains valid, asks the target adapter to perform its effects, and then
commits both the shell state and visible composition. An effect failure restores the previous valid
composition. Cleanup failures enter an explicit fault state for recovery and diagnostics.

Screens and overlays use the same lifecycle transaction. Focus handoff, callback and timer cleanup,
queued-intent removal, and refresh-intent changes participate in that transaction. Navigation and
overlays remain independent state so an overlay can compose over any compatible base screen.

Async requests and replies carry request and surface-instance identity. The coordinator applies a
reply when its identity matches the current request and instance generation. Network, storage,
sensor, radio, audio, and other long-running work completes through later correlated events instead
of holding a visible transaction open.

Each product supplies a target adapter for LVGL, display refresh, persistence, and product policy.
Subsystem engines and hardware resource owners keep their existing lifecycles and expose bounded,
typed messages to the coordinator. The display owner remains responsible for panel transactions and
selects refresh mode from dirty output and merged refresh intent.

All queues, stacks, overlay sets, retained models, and effect batches use fixed capacities. Capacity
changes follow the DRAM budget review.

Medinote will replace its separate shell commit and LVGL rebuild with coordinator transactions.
Meditamer will move its reusable coordination logic into the shared coordinator while retaining its
Inkplate-specific renderer and refresh adapter.

## Consequences

- Both products use the same navigation, overlay, focus, ordering, and failure semantics.
- Independent navigation and overlay state supports layered compositions without expanding a
  product-wide combined-state enum.
- Logical shell state and visible LVGL lifecycle commit or roll back together.
- Request and instance identities provide a common rule for rejecting stale async results.
- The coordinator can be tested with a fake target adapter, including failure injection at each
  effect.
- The shared contract adds event, transaction, effect, and adapter types that both products must
  integrate.
- Migrating Meditamer's mature backend requires parity traces for commit, rollback, cleanup faults,
  modal scheduling, and ambient fallback.
- Serial visible transactions can defer later UI changes while target effects run, so target effects
  stay bounded and long-running work returns through correlated events.
- Fixed capacities require measurement and per-target tuning against static and runtime memory
  budgets.
- Product-specific rendering, refresh policy, and hardware lifecycles remain in target adapters.

## Alternatives considered

- **Give Medinote a transactional backend of its own, modeled on Meditamer's, instead of sharing
  one.** Rejected: it duplicates the same event-ordering, focus, and failure-recovery logic ADR-0015
  already extracted once from Meditamer into `platform/shell`. A second bespoke implementation would
  drift from the first the same way Medinote's separate commit/rebuild steps already drifted from
  Meditamer's transactional one, reopening the sync gap this ADR exists to close.
- **Let each product keep committing shell and LVGL state in separate steps, and only tighten
  Medinote's ordering.** Rejected: the two-step shape is the defect, not an implementation detail of
  it -- any failure between the steps still leaves logical and visible state out of sync, and every
  future overlay or background subsystem would have to reason about that window itself.
- **Model the shared contract as a single combined enum over both products' UI states.** Rejected:
  navigation and overlay state must stay independent so an overlay can compose over any compatible
  base screen (this ADR's design already requires that); a combined enum would multiply with every
  screen/overlay pair added on either product, which is what ADR-0007's separate navigation and
  overlay stacks avoided in the first place.

## Phase 0 design: access token, widget ownership, cleanup API, and failure recovery (2026-08-28)

The initial implementation work defined the LVGL access token, widget ownership model, and cleanup
API this decision's "renderer-independent
`UiCoordinator`" and "shared LVGL adapter" (Phase 1) rest on, and the recovery rules for a cleanup
fault and Medinote's rollback path. This section records those decisions, made and proven against a
real implementation: the Phase 1 safety slice in `platform/render/src/lvgl_adapter.rs`, covering
Meditamer's Home screen and its sticky refresh-control overlay (see the archived
[evidence ledger](../archive/refactors/shared-ui-coordination-ledger-2026-08-29.md)).

**LVGL access token.** One UI task drives LVGL; a `UiAccessToken` is proof of being that task. It
carries a `runtime_id` set by an `unsafe fn issue()` the task calls once, immediately after `lv_init`
(directly, or after a full deinit/reinit cycle) -- matching `InitializationSession`'s existing
boundary in `backend/init.rs`. Every adapter operation takes a token by reference; the token itself
carries no pointer, so passing it around freely (it is `Copy`) cannot leak LVGL access to another
task. This is the same "proof of the right context" shape as `shell`'s existing `ProviderToken`, one
level up: a `ProviderToken` proves a request came from a still-registered provider, and a
`UiAccessToken` proves a call is happening on the runtime that is allowed to touch LVGL at all.

**Widget ownership model.** Each live LVGL object is represented by one owned `Widget` handle
(`Timer` follows the identical shape for `lv_timer_t`). Two mechanisms enforce ownership together:

- *Types, for "exactly one owner deletes it".* `Widget` is not `Clone`/`Copy`; only the value a
  `create`/`child` call returns can ever reach `delete`, which consumes it. This is enforced entirely
  at compile time -- no runtime check is needed for a widget nobody else can name.
- *A checked handle, for "the object this handle names is still the one LVGL has".* A `Widget` also
  carries the address it was created at and a generation drawn from a single global, per-kind
  counter -- the same shape as `shell::types::SurfaceInstanceToken`'s generation, chosen for the same
  reason: LVGL's allocator can hand a freed object's address to an unrelated new one, so a handle's
  validity has to ride on a number that is never reused, not on the address alone. Every operation
  checks the token's `runtime_id` and the handle's generation against a small runtime registry before
  it touches LVGL. Parent/child lifetime is covered by the same mechanism from the parent's side:
  deleting an object walks its LVGL child tree first and removes each descendant's registry entry, so
  a child handle held elsewhere becomes rejected (not dangling-and-usable) the instant its ancestor is
  torn down -- before that handle's next operation can reach LVGL, not after.

Callback and timer registrations are owned by the surface that installed them, through the existing
`render::intent_bridge` contract: a callback only enqueues a typed action, which the UI owner applies
after the LVGL call that produced the event has returned. The adapter's `Widget::on_event`/`on_click`
just add the checked-handle guard in front of `lv_obj_add_event_cb`; they do not change that contract.

**Cleanup API that reports failures.** `Widget::delete`/`Timer::delete` consume `self` and return
`Result<(), _>`. Three outcomes exist, matching `shell::lifecycle::DestroyFailure` one level below
it: deleted (registry entry freed); the handle was already invalid, so nothing was touched
(`WidgetDeleteFailure::AlreadyGone`); or LVGL reported the object still valid after deletion, in which
case the widget is handed back so the caller can retry (`WidgetDeleteFailure::StillValid(Widget)`).
Callers above the adapter fold this into their own type the same way: `HomeScreen::destroy` and
`RefreshControl::destroy_root` each return `Result<(), Self>`, so a still-valid failure hands back a
reconstructed, still-usable screen or overlay rather than the adapter's internal error shape.

**Recovery from a cleanup fault.** A `StillValid` failure does not corrupt the registry or block
retry: the handle's entry is untouched (only removed on confirmed deletion), so calling `delete`
again later, once whatever kept LVGL from actually removing the object clears, works normally. This
matches the existing pattern one level up: `Backend::cleanup_blocked`/`overlay_cleanup_blocked` retain
a `DestroyFailure::Live` instance and retry it on the next `drain_navigation` pass
(`backend/overlay.rs`'s `retry_blocked_cleanup`/`retry_overlay_cleanup`); Home's and the refresh
overlay's `StillValid` failures now feed that same retry path through `ActiveSurface::destroy`'s and
`ActiveOverlay::destroy`'s existing `DestroyFailure::Live` branches, unchanged.

**Medinote's rollback path.** Medinote's `runtime_ui` (`targets/medinote-waveshare/src/runtime_ui.rs`)
commits shell navigation and rebuilds LVGL in two separate steps today; nothing in this Phase 0/1
slice touches Medinote's code. The rollback path Phase 3 (Medinote migration) must provide is fixed by
this decision's failure flow, though: once Medinote's `SurfaceRuntime` adapter routes its screen
construction through the same checked-handle contract, a construction failure rolls back exactly as
`shell::lifecycle::execute_transition` already drives it for Meditamer -- restore the origin instance
active, tear down the failed candidate (through its own `delete`, so a `StillValid` failure there
becomes a `cleanup_blocked` instance retried the same way), and leave the previously visible surface
usable throughout. Phase 3 supplies the adapter; it does not need a different failure rule.

## Input ownership clarification (2026-08-28)

Local buttons, touch and BLE controls use the same UI-owner routing and surface-lifecycle
rules. Acquisition and device decoding retain their existing owners; products choose bindings.
Gamepad and physical buttons expose press/release independently of optional click or gesture
recognition. Touch retains contact Down/Move/Up/Cancel semantics for consumers needing those edges.

The UI owner applies modal capture and routes input to LVGL or app/product handlers. Resulting
navigation and composition use coordinator transactions; individual input samples need no visible
transaction. Cancellation ends held interactions across source resets and ownership changes.
This clarifies the existing typed-product boundary without requiring a new input runtime.

## `UiAccessToken` capability tightening (2026-08-29)

The Phase 0 design section above records `UiAccessToken` as `Copy`, with the rationale that "the
token itself carries no pointer, so passing it around freely ... cannot leak LVGL access to another
task." The ui-runtime-finalization plan's Phase 1 revisits that: a capability proving "the holder is
the UI task driving the current LVGL runtime" should not duplicate itself implicitly or cross to
another task at all, even though nothing it carries is itself unsafe to copy. `UiAccessToken` is now
`Clone` (not `Copy`) and carries a `PhantomData<*const ()>` marker making it `!Send`/`!Sync`, the same
`!Send` shape `Widget`/`Timer` already had from their own raw-pointer fields. Every call site that
relied on implicit duplication now clones explicitly (`platform/render/src/lvgl_adapter.rs`,
`products/meditamer/src/firmware/ui/lvgl/backend*.rs`) -- the compiler surfaced each one as an
E0507 move error, so this is confirmed exhaustive.

This still stops short of a fully affine (non-`Clone`) single-owner token: `Backend` and its
`SurfaceRuntime`/`CompositionRuntime` adapters each hold their own copy today, and making the type
genuinely non-`Clone` would require restructuring that ownership (threading `&UiAccessToken`
everywhere instead) beyond what the finalization plan's Phase 1 scoped. That restructuring, if
wanted, belongs to this ADR's own Phase 5 UI-checked-boundary work rather than a further amendment
here.

`UiAccessToken::issue()` also now clears the checked-handle registries (`WIDGET_REGISTRY`,
`TIMER_REGISTRY`), so a genuine LVGL reinit cycle cannot leave stale `(addr, generation)` entries
occupying registry capacity forever (individual stale handles were already rejected correctly by
their `runtime_id` check; only the registry-space reclamation was missing). `issue()`'s existing
safety contract -- the caller must have just performed a real `lv_init`/reinit -- already made this
safe in the crate's one production call site (`backend/init.rs`); the adapter's own test needed
updating to build a second, mismatched-`runtime_id` token directly rather than via a second
`issue()` call, since that call no longer models a live-session no-op the way it used to.

## Reconciling the Decision with what was built, and closing the checked-LVGL boundary (2026-08-29)

The ui-runtime-finalization plan's Phase 5 converted every remaining first-party presentation root
on both products (Meditamer's Launcher/Ambient-picker/Overlay-toggles catalogue presenter, gesture
diagnostics, Ambient View, the shared carousel widget, the base overlay set -- passive navigation
cue, Confirm, Settings, and the sticky refresh control already converted -- and the provider-removal
fixture; Medinote's own screens were already checked-adapter-built as of Phase 2) onto
`platform/render/src/lvgl_adapter.rs`. Closing that boundary is also the occasion to correct four
places where this ADR's Decision section describes a contract that was never built, or omits one
that was. None of these are behavior changes; they are the ADR catching up to the implementation
that shipped instead of it.

**The four-type contract in "Decision" does not exist.** That section names `UiEvent<P>`,
`UiTransaction`, `UiEffect`, and `UiEffectResult` as the shared interaction contract. Only
`UiEvent<P>` was built (`platform/shell/src/coordinator/mod.rs`). `UiTransaction`/`UiEffect`/
`UiEffectResult` appear nowhere in `platform`, `products`, or `targets`. What shipped instead is a
pair of synchronous traits called directly rather than an effect-batch-and-result envelope:
`SurfaceRuntime` (`platform/shell/src/lifecycle.rs`) for screens and `CompositionRuntime`
(`coordinator/mod.rs`) for overlays, with outcomes modeled as `UiDispatchResult`,
`NavigationDispatchOutcome`, `CompositionDispatchOutcome`, and `CompositionRollbackReason` rather
than a generic `UiEffectResult`. `UiCoordinator::dispatch`/`dispatch_navigation`/
`dispatch_composition`/`dispatch_provider_removal` (the last split into its own
`coordinator/provider_removal.rs` file, since it cannot reuse `execute_transition`'s timing the way
the other two do -- see that file's own doc) are the real shape this ADR's "asks the target adapter
to perform its effects, and then commits" describes. Treat `SurfaceRuntime`/`CompositionRuntime` plus
these outcome enums as this ADR's actual Decision from here on; a future ADR should retitle or
supersede the ones that never shipped rather than this amendment carrying that weight indefinitely.

**Async request/reply identity correlation was never built.** The Decision section states "Async
requests and replies carry request and surface-instance identity. The coordinator applies a reply
when its identity matches the current request and instance generation." Only the second half is
real: `SurfaceInstanceToken`'s generation and `ProviderToken`'s generation are checked throughout
`shell`. There is no request-id or correlation type anywhere in `platform/shell` -- no `RequestId`,
no `request_id` field, no matching machinery. Every actual async completion path in this codebase
(wall-clock sync, observation delivery, BLE) correlates through instance/provider generation alone,
without a separate request identity, and that has been sufficient in practice. This paragraph is
aspirational until a real caller needs to distinguish two in-flight requests against the *same*
instance generation; until then, note it as unimplemented rather than implying it exists.

**`Timer::delete` does not share `Widget::delete`'s three-outcome shape.** The Phase 0 section
above describes `Widget::delete`/`Timer::delete` together as returning the same
deleted/`AlreadyGone`/`StillValid` three-outcome `Result`. Only `Widget::delete` does
(`WidgetDeleteFailure`, `platform/render/src/lvgl_adapter.rs`). `Timer::delete` returns
`Result<(), AccessFault>` -- no retry path, because nothing in this codebase has yet needed one (no
screen or overlay converted in Phase 1 or Phase 5 creates a `Timer`; the type exists for a future
periodic-repaint use this repo has not reached). Give `Timer::delete` the matching three-outcome
shape when a real caller needs a still-valid timer handed back for retry, rather than assuming it
already has one.

**`Widget::as_raw` is part of the real contract, not an unstated escape hatch.** The Phase 0 section
asserts raw pointers stay private to the adapter. In practice, both products depend on `as_raw` for
screen/overlay activation (`lightvgl_sys::lv_screen_load`) and for the lifecycle glue that swaps a
visible root or deletes across surface kinds through one common pointer
(`Widget::as_raw`'s own doc, `ActiveSurface::root`/`activate_root` in Meditamer's `backend.rs`, the
equivalent in Medinote's `runtime_ui/mod.rs`). This is deliberate, not a leak: the checked adapter
has no activation or cross-kind-teardown operation of its own, because that is coordinator/backend-
level policy (screen-swap timing, panel refresh sequencing), not a per-widget concern the adapter
should own. `as_raw`'s existing doc comment already says this; this amendment records it here too so
the Decision section's "raw pointers... private to this module" claim does not read as contradicted.
Whether the adapter should eventually grow its own activation primitive (removing the need for
`as_raw` at these call sites) is an open question for a later pass, not one this Phase resolved --
both products' target event pumps still perform this one raw `lv_screen_load`/refresh sequence
directly, by design (see the ui-runtime-finalization plan's own Phase 4 progress note on Medinote's
side).

Two stale references, corrected in code rather than here: `backend.rs`'s `Backend::ui_token` field
comment described the token as `Copy` and scoped to "Home's and the sticky refresh-control overlay's"
behalf -- both stale since the 2026-08-29 `Clone`/`!Send` tightening and this phase's full
conversion; and `scripts/ci/check_ui_shell_ownership.sh` names a `products/meditamer/src/firmware/ui/
screen/overlay_settings.rs` that does not exist (Settings lives in `overlay/base_overlays.rs`) and
never checks `catalogue_presenter.rs` or `widget/carousel.rs`, where the raw LVGL for Launcher and
gesture-test actually lived before this phase -- fixing that script's own surface list is
`ui-runtime-finalization`'s Phase 6 work, not this ADR's. (Corrected, 2026-08-29: the script no
longer names `overlay_settings.rs` and already checks both `catalogue_presenter.rs` and
`widget/carousel.rs` -- this paragraph itself had gone stale in the direction it warned about.)
