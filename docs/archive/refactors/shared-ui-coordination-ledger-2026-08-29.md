# Shared UI coordination evidence ledger

- Status: Done — retained as dated implementation evidence
- Last-reviewed: 2026-08-29

Entries retain their original phase names and conclusions as dated evidence. Current
work and acceptance are outside this ledger.

## Ledger Rules

- Keep scope, phase status, acceptance checks and required follow-ups in the plan. Add each necessary
  follow-up to the relevant phase with a completion condition; link to it from the finding here.
- Add an entry when a slice completes or a finding changes what is needed for acceptance. Keep
  routine progress, individual commands and retries in working artifacts.
- Use one short entry per result: stable ID, date, phase link, finding or outcome, evidence links
  and limits of the evidence. Identify host tests, device traces and physical checks separately.
- Store full commands, logs, source/build manifests and measurement tables in artifact files. Link
  to the relevant artifact and summarize the conclusion here.
- Record each result once. Reuse entry links when later phases depend on the same evidence.
- At phase closure, compact repeated details and superseded conclusions. Preserve entry IDs,
  artifact links and the failure-to-fix history needed to understand the result.
- Keep the ledger focused on required acceptance evidence. Archive it with the plan when the work
  is complete.

## Evidence Entries

### E-0001 -- Phase 0: design agreed

ADR-0017 accepted; its "Phase 0 design" section (2026-08-28) records the `UiAccessToken` contract,
the `Widget`/`Timer` ownership model (Rust ownership for "exactly one owner deletes it", a
generation-checked handle for "the object this handle names is still the one LVGL has"), the
`Result`-returning cleanup API and its three outcomes, the cleanup-fault retry rule (unchanged from
`Backend::cleanup_blocked`/`overlay_cleanup_blocked`), and Medinote's rollback path (fixed by
`execute_transition`'s existing failure flow once Phase 3 supplies the adapter). No code; the design
was cross-checked against the Phase 1 implementation below before being recorded as accepted.

Limits: a design review, not device evidence. Reused by Phase 1 and every later phase's failure
handling.

### E-0002 -- Phase 1: LVGL safety boundary, host evidence and both target builds

Added `platform/render/src/lvgl_adapter.rs`: `UiAccessToken`, checked `Widget`/`Timer` handles per
E-0001's design, and the style/geometry/callback operations Home and the sticky refresh-control
overlay use. Converted `products/meditamer/src/firmware/ui/screen/home.rs`,
`widget/ambient_content.rs`, and the new `overlay/refresh_control.rs` (extracted from
`base_overlays.rs`, which keeps the passive and confirm-modal overlays on the raw LVGL API) to the
adapter; all three now carry `#![forbid(unsafe_code)]`. `widget/carousel.rs` stays unconverted and
unsafe -- it is also used by `gesture_test` and `catalogue_presenter`, outside this slice's scope, so
Home's carousel navigation is a private, adapter-based copy inside `home.rs` rather than a shared
half-migrated helper. `ActiveSurface::destroy`/`ActiveOverlay::destroy` (`backend.rs`/
`base_overlays.rs`) special-case Home and the refresh overlay to call their own checked `destroy`
instead of the generic raw `lv_obj_delete` path, so the adapter's registry entries are freed instead
of leaking on every cleanup; every other surface/overlay kind is unaffected. `UiAccessToken` is issued
once in `InitializationSession`'s existing boundary (`backend/init.rs`, right after `lv_init`) and
threaded through `Backend`, `LvglSurfaceRuntime`, and the free `destroy_initial_*` helpers.

Host evidence (`bash scripts/host-test.sh test render` / `lint render`, aarch64-apple-darwin, real
LVGL via `DEP_LV_CONFIG_PATH`): one test, `lvgl_adapter::tests::safety_boundary`, covers construction
shaped like Home's button/label tree and the refresh overlay's system-layer button; queued navigation
from a callback bound through the adapter to `intent_bridge::navigation_callback`, verified end to end
via `take_intent`; a timer created and deleted through the same checked-handle contract; a wrong-
runtime handle rejected before touching LVGL, with the original token still working afterward;
parent deletion invalidating every live descendant handle (`Stale`) and the deleted widget itself
(`AlreadyGone`) before LVGL is touched again; registry exhaustion during a candidate build
(`RegistryFull`) with a separately-tracked "active" widget staying fully usable throughout, matching
`shell::lifecycle::execute_transition`'s previous-screen-stays-usable contract; and LVGL's allocator
integrity (`lv_mem_test`) confirmed clean at the end. Compile-fail/lifetime rules are enforced by
`Widget`/`Timer` not implementing `Clone`/`Copy` and `delete` consuming `self` -- ordinary Rust
ownership, not a separate compile-fail test suite. `scripts/host-test.sh lint render` (Clippy, deny
warnings) passes; two call sites needed a documented, precedented
`#[allow(clippy::not_unsafe_ptr_arg_deref)]` (the FFI `user_data` pointer is forwarded, never
dereferenced, by this function).

Both `targets/meditamer-inkplate` target builds pass end to end (`-Zbuild-std=core,alloc`,
`xtensa-esp32-none-elf`): debug (`minimal` profile) and debug with `ui-provider-fixture` and
`ui-initialization-fixture` both enabled (`default` profile plus those features -- debug builds need
`CARGO_INCREMENTAL=0` here; incremental compilation breaks `xtensa-lx-rt`'s `LBEG` asm), release
(`minimal` profile), and Clippy (`-D warnings`) with both fixture features enabled. No new warnings
from the changed code; one pre-existing, unrelated `dead_code` warning
(`observability::types::Snapshot::wifi_ipv4`) was already present.

Limits: host tests and host/target compiles only, run 2026-08-28. Not covered: physical-device
evidence (see E-0003, below); the `ble-release` profile and
`all-features`/`telemetry-defmt`/`shared-ble-runtime` feature combinations.

### E-0003 -- Phase 1: physical-device evidence on Inkplate

On Inkplate `e8:6b:ea:fb:d5:54` (`/dev/cu.usbserial-8310`), via the canonical
`scripts/device/flash.sh debug` full-flash-and-boot-capture workflow (`config/partitions-single-
production.csv`, production boot target).

**Plain debug boot** (default features, diagnostic ELF
`fbf8e2c2ae429b32060fb2bc3a31460111c3b298118d6957905b6caacf36a0ad`): normal boot reached
`UI_SETTINGS_BOOT base=home state=entered` (Home entered through the adapter), then
`LVGL_LIFECYCLE phase=candidate_created ... integrity_ok=true` immediately after Home's
construction. The default startup entry then navigates Home -> Launcher -> Ambient view; the
Home -> Launcher step destroys Home through `ActiveSurface::destroy`'s new checked-`Widget::delete`
path, and `LVGL_LIFECYCLE phase=settled_after_delete ... integrity_ok=true ... cleanup_blocked=false`
confirms it completed cleanly. Startup finished at `LVGL init=ready ... startup_rendered=true`,
`UI_STATE screen=ambient_view state=entered`.

**Fixture boot** (`ui-initialization-fixture` feature, diagnostic ELF
`53f69f06eb5c0c17356023812700d422bd7fe1d4ca656c907549e19de4d660a3`): `Backend::verify_initialization`
(`backend/initialization_fixture.rs`) ran before ordinary boot, on the same code path E-0056 recorded
for the pre-adapter Backend. All 13 `UI_INIT_FIXTURE ... outcome=passed` markers passed: 2 rounds of
5 injected-failure stages each (`Session`, `Display`, `Input`, `NavigationOverlay`, `RefreshOverlay`),
plus `installed_callback_and_teardown` and `stale_callback_and_reinitialization`. The `RefreshOverlay`
stage is the one this slice changed: the sticky refresh-control overlay is fully constructed through
the new `refresh_control.rs`/adapter path, then the fixture's injected failure forces
`destroy_initial_backend_or_stop` to roll it back through `ActiveOverlay::destroy`'s new
`RefreshControl::destroy_root` path (and Home through its own new path) before `assert_clean` checks
LVGL is fully deinitialized, `LVGL_DISPLAY`/`LVGL_INPUT` are null, external (PSRAM) free bytes are
back at baseline, and no callback intent survived -- i.e., a real cleanup-fault rollback, on-device,
through the adapter's checked-handle `delete`. `stale_callback_and_reinitialization` additionally
reinitializes the backend a second time (a fresh `UiAccessToken::issue()`, a second LVGL runtime
epoch) and asserts `lv_mem_test() == LV_RESULT_OK` afterward. Ordinary boot then proceeded
identically to the plain-debug run above, and a real partial refresh completed afterward
(`LVGL_REFRESH phase=service ... status=ok kind=partial`).

Both captures: no panic, no Guru Meditation, no `integrity_ok=false`, no `cleanup_blocked=true`,
anywhere in ~10-24 KB of boot output. Absolute resource readings at Home's first construction (plain
boot): `lvgl_used=13784` of `lvgl_total=128252` bytes, `heap_internal_free=27860`,
`heap_external_free=3916528`, `cpu0_stack_min=101488` -- all comfortably healthy, consistent with a
behavior-preserving refactor (same LVGL calls, same order, only their Rust-side access made safe).

The device was left flashed with the plain-debug build afterward (re-confirmed booting clean:
`LVGL init=ready`, `UI_STATE screen=ambient_view state=entered`, no panic) rather than the
fixture build, which reruns `verify_initialization` -- full-flash-and-boot-capture cycles, not a
one-shot check -- on every subsequent boot.

Limits: the fixture's `click_route` synthesizes a click rather than a physical touch (superseded by
E-0004, below, for that gap specifically). No differential memory/timing comparison against a
pre-Phase-1 baseline build was taken (only the absolute post-Phase-1 numbers above); the change is
Rust-side access safety only, not a change to which LVGL calls run or in what order, so no regression
is expected, but it was not measured against a baseline build. Both captures used the
`default`/`minimal`+fixture profiles only, matching E-0002.

### E-0004 -- Phase 1: physical touchscreen taps on Home, live session

Same device as E-0003, on the plain-debug build E-0003 left it on, with a live `espflash monitor`
(`-p /dev/cu.usbserial-8310 --before no-reset-no-sync --after no-reset --non-interactive`, no reset)
attached and the user physically tapping the touchscreen while Claude read the serial log after each
tap. `raw`-mode monitoring (`tio`/`stty`+`cat`) was tried first and failed non-interactively in this
harness (`tio` exits immediately in its own "non-interactive mode"; `stty` warned `stdin isn't a
terminal` and the port then read at the wrong rate) -- plain `espflash monitor` with an explicit
port and `--non-interactive` worked cleanly instead.

Sequence, each step confirmed against the live log before moving to the next: Ambient view -> tap
background (reveals the time display's Back button) -> tap Back -> `UI_NAV ... to=Launcher
role=Launcher ... integrity_ok=true`; Launcher -> tap either carousel arrow (both bound to Home) ->
`UI_NAV ... to=Home role=Ambient ... integrity_ok=true`; **Home -> tap "TOP TEST" -> `UI_NAV
state=committed from=Home to=Launcher role=Launcher outcome=Changed ... cleanup_blocked=false`**,
with the preceding `candidate_created`/following `settled_after_delete` `LVGL_LIFECYCLE` lines both
`integrity_ok=true` -- a real finger tap through the touch digitizer, LVGL's real hit-testing and
event dispatch, into a button built entirely by `Widget::child`/`Widget::on_click`, through
`intent_bridge::navigation_callback`, into `shell`'s navigation, committing a transition that
destroys Home through `ActiveSurface::destroy`'s new checked-`Widget::delete` path -- end to end, on
hardware, from a physical touch; back to Home (Launcher -> arrow -> Home, `UI_NAV` confirmed again)
and **tap a carousel arrow -> `UI_NAV state=committed from=Home to=Launcher role=Launcher
outcome=Changed`**, confirming the carousel buttons (built with different style calls --
`set_ext_click_area`, `Font::Size32` -- than the top button) are also live and correctly wired.

No panic, no `integrity_ok=false`, no `cleanup_blocked=true`, across the whole session. This closes
the physical-touch gap E-0003 left open: all three of Home's interactive widgets (the top button and
both carousel arrows) now have a real-finger-tap confirmation, cross-checked against the backend log
every time, not just a synthesized click.

Limits: the sticky refresh-control overlay was not interacted with live (E-0003's fixture already
covers its construction and cleanup-fault rollback on-device; this session only exercised Home's
navigation). The differential memory/timing comparison against a pre-Phase-1 baseline remains open,
as recorded under E-0003.

### E-0005 -- Phase 2: `UiCoordinator`, host evidence against a fake renderer

Added `platform/shell/src/coordinator.rs`: `UiCoordinator<Screen, Overlay, PROVIDERS, SURFACES,
NAVIGATION, OVERLAYS, MODALS, INTENTS>` wraps `ShellModel` plus the active screen and live-overlay
instances it governs. `CompositionRuntime` is a new trait -- the same enter/show/hide/enable/
disable/destroy shape as `crate::lifecycle::SurfaceRuntime`, renamed for overlays and kept separate so
one concrete adapter can implement both with different `Instance` types. `dispatch_navigation`
extends `crate::lifecycle::execute_transition` (reused unchanged for the screen swap itself) to also
stage the bundled composition delta every navigation carries (a promoted queued modal can enter, a
dropped transient can leave, in the same transaction). `dispatch_composition` and
`dispatch_provider_removal` generalize `Backend::enter_overlay`/`remove_live_overlay` and the
provider-detach path into a renderer-independent `stage entries -> stage departures -> commit ->
complete departures + activate entries` sequence -- the exact order E-0002/E-0003/E-0004 already
proved on real Inkplate hardware for the concrete LVGL case, not a new design. `UiEvent<P>` is the
unified dispatch entry point (`Navigate`/`Compose`/`Product(P)`); the coordinator never interprets
`Product(P)` itself.

Host evidence (`bash scripts/host-test.sh test shell` / `lint shell`, aarch64-apple-darwin): a fake
`SurfaceRuntime`/`CompositionRuntime` pair with per-call failure-injection knobs (mirroring
`crate::lifecycle::tests::FakeRuntime`), and 10 new tests covering every Phase 2 acceptance case
directly: navigation depths (Home -> Launcher -> AppRoot -> AppChild -> Back -> Home, checking
`ShellModel`'s own navigation-stack depth partway through); empty and populated overlay admission;
passive and interactive overlay kinds both admitting live; an active modal admitting live, a second
modal queuing behind it, and dismissing the first promoting the second into the live slot with a
fresh `enter` (not a stale reuse of the first's instance); provider removal falling back to Home and
finalizing with `ProviderRuntimeAudit`; a stale composition source (`OwnedCompositionIntent` built
from an instance that navigated away) rejected by `CompositionError::InvalidSource`, not silently
attributed to whatever is active now; an overlay capacity limit (`OVERLAYS = 1`) reported through
`CompositionError::Reference(LiveOverlayCapacity)` with the runtime never seeing a second `enter`,
not corrupting the composition it already holds; an intent queued against the active instance while
an unrelated navigation runs to completion first, then found purged (not silently applied against
the wrong, now-active screen) when drained -- the coordinator's "events during a change" case, given
target effects here are synchronous so the real race is queue staleness, not concurrent dispatch; a
failed overlay `enable` rolling the candidate back through `destroy` without the shell revision
moving; and a screen `destroy` failure surfacing as `FaultedAfterCommit` with `is_faulted()` true,
`dispatch_navigation` then refusing further work (`DispatchRejected::Faulted`) until
`retry_blocked_cleanup` clears it. All existing `shell` tests (46) continue to pass; Clippy
(`-D warnings`) is clean.

Limits: proven against a fake renderer only -- no product (Meditamer, Medinote) wires
`UiCoordinator` in yet, so the `Screen`/`Overlay` generic split and the `SurfaceRuntime`/
`CompositionRuntime` trait shapes are unproven against a second, real adapter (Phases 3/4). Focus and
the merged refresh hint are pure queries, not dispatched `UiEffect::Focus`/`UiEffect::Refresh`
effects (see the plan's Phase 2 note for why). No host-buildable "real LVGL" evidence applies here --
unlike Phase 1's adapter, this module has no LVGL dependency at all, so there is no device evidence to
gather until a product migration reaches it.

### E-0006 -- Phase 3: Medinote migrated, real Waveshare S3 evidence, one bug found and fixed

Added `MedinoteScreenRuntime` (`shell::lifecycle::SurfaceRuntime`) and `MedinoteOverlayRuntime`
(`shell::coordinator::CompositionRuntime`) to `targets/medinote-waveshare/src/runtime_ui.rs`. One
screen adapter covers Home, Launcher, and Hourglass: Medinote draws every navigable surface onto one
persistent LVGL screen object (`clear_screen` + rebuild in place), unlike Meditamer's per-surface
objects (ADR-0017 Phase 1), so `enter` does the clear-and-rebuild and `activate`/`quiesce`/`enable`
are trivially `true` and `destroy` trivially `Ok(())` -- there is nothing to separately show, hide, or
free per instance with one persistent object. The overlay adapter wraps the already-standalone
`BleStatusWidget` (a real `lv_layer_top()` object, unlike the screens). `RuntimeShell` (raw
`ShellModel`) became `RuntimeCoordinator` (`UiCoordinator<MedinoteScreen, MedinoteOverlay, ...>`); the
`navigate` helper and `install_ble_status_overlay` now call `dispatch_navigation`/
`dispatch_composition` instead of committing the shell and rebuilding LVGL as two separate steps --
concretely, the LVGL rebuild now runs *before* the shell commits (`execute_transition`'s existing
ordering), fixing the divergence-on-failure gap ADR-0017 names for Medinote specifically, in the
direction Meditamer's adapter already established. `key_navigation_intent`/`boot_navigation_intent`
(pure `SurfaceRole -> Option<NavIntent>` functions) are the product action mapper for KEY/BOOT's
navigation meaning; `crate::input`'s physical button recognition is untouched. `UiCoordinator` gained
`live_overlay`/`live_overlay_mut` (by `SurfaceInstanceToken`, resolved through
`CompositionRuntime::instance`) -- a real gap Phase 2's fake-renderer tests didn't surface, needed for
CheerTok's per-frame status-label update, which is not itself a lifecycle transition.

Host evidence: `targets/medinote-waveshare` has no host test suite (xtensa-only, not in
`scripts/host-suites.tsv`; its own `#[cfg(test)]` module in `input.rs` is unreachable today --
`[[bin]] test = false`). The real build/lint gates instead: `bash build.sh` (release,
`-Zbuild-std=core,alloc`, `xtensa-esp32s3-none-elf`) and `cargo clippy -- -D warnings` both pass
clean, no warnings from the changed code.

Physical-device evidence, Waveshare S3 (`a4:cb:8f:d0:6a:74`, `/dev/cu.usbmodem21101`), via
`targets/medinote-waveshare/flash.sh` (diagnostic ELF
`e4e10b78eeb5d60260b8bde3673c18d52f126c030cf1b6e71ed0839c05bf71ae`): boot capture showed
`UI_SURFACE role=Ambient` (Home entered through the coordinator's `bootstrap_screen`),
`RUNTIME_READY`, `HOME_ENTRY_REQUEST provider=BATTERY|ENVIRONMENT outcome=Admitted` (the observation
handoff against `coordinator.shell().active_instance()`), and `CHEERTOK_UI state=Stopped` (the
overlay admitted through `dispatch_composition` and its live per-frame update reaching the widget via
the new `live_overlay_mut` accessor) -- no panic, no Guru Meditation. A live `espflash monitor`
session (attached with no reset, matching E-0004's Inkplate method) with the user physically pressing
KEY/BOOT on the board, each press confirmed against both the live serial log and the visible screen:
KEY on Home -> `UI_SURFACE role=Launcher` (**launcher not visible on the panel the first time** --
see the bug below); after the fix, KEY on Home -> Launcher, KEY on Launcher -> `UI_SURFACE
role=AppRoot app=hourglass`, BOOT -> Launcher, BOOT -> Home, every step confirmed on-screen and in the
log, no panics across the whole session.

**Bug found and fixed, pre-existing and unrelated to this migration's own correctness (user's
assessment, matching the analysis below):** the first live KEY press committed the shell to Launcher
(`UI_SURFACE role=Launcher` logged correctly) but the launcher never appeared on the panel -- the
display kept showing Home. Root cause: `service_lvgl`'s `lv_tick_inc(1)` (used at every screen
rebuild before this fix, including in the pre-Phase-3 code this replaced) advances LVGL's simulated
clock by a hardcoded 1 ms regardless of real elapsed time, while `lv_conf.h` sets `LV_DEF_REFR_PERIOD`
to 30 ms; `lv_timer_handler` can legitimately decide the display's periodic refresh timer isn't due
yet under its own simulated clock and skip the flush, and nothing else re-services LVGL until the
next navigation event, so a skipped flush there left the previous screen showing indefinitely. Fixed
by calling `refresh_screen_full_now` (`lv_refr_now`, unconditional, bypassing the period gate) inside
`MedinoteScreenRuntime::enter` instead of `service_lvgl(1)` -- already the established pattern at
every other full-screen rebuild in this same file (sleep, deep sleep, wake); it simply wasn't applied
to ordinary navigation before. Rebuilt, reflashed (same ELF hash workflow, new build), and the full
KEY/BOOT round trip above is the confirmation this fixed it. The CheerTok per-tick status update's own
`service_lvgl(1)` call was not changed -- same theoretical risk, but it retries every loop iteration
rather than only at a discrete transition (so a skipped flush there is laggy, not stuck), and nothing
confirmed it broken.

Limits: sleep, deep-sleep, and the sleep/wake transition screens are deliberately outside the
coordinator (see the plan's Phase 3 follow-up for why) and were not touched or newly verified here.
CheerTok/BLE was only exercised through the no-device-found path, not an actual pairing. No
differential memory/timing comparison against a pre-Phase-3 baseline was taken.

### E-0007 -- Phase 4: Meditamer migrated, including the provider-fixture removal path, two real bugs found and fixed

`Backend` (`products/meditamer/src/firmware/ui/lvgl/backend.rs` and its `init`/`navigation`/`overlay`/
`frame`/`cycle` submodules) now owns one `MeditamerCoordinator` (`UiCoordinator<ActiveSurface,
ActiveOverlay, ...>`) instead of a raw `ShellModel` plus its own `active`/`cleanup_blocked`/`overlays`/
`overlay_cleanup_blocked`/fault-flag fields. `LvglSurfaceRuntime` (unchanged) and a new
`LvglOverlayRuntime` (`shell::coordinator::CompositionRuntime`) are the two target adapters;
`LvglOverlayRuntime::enter` infers which of NavigationCue/RefreshControl/Confirm to build purely from
`instance.token.surface` identity against `self.surfaces`, unifying what `create_overlay_candidate`
(Confirm only) and `install_base_overlay` (an explicit `BaseOverlayKind`) used to decide separately.
Per the user's explicit direction, this migration includes the `ui-provider-fixture` removal harness
(`backend/cycle.rs`) -- Meditamer's first and only real-hardware exercise of
`UiCoordinator::dispatch_provider_removal`/`finalize_provider_removal`, since Medinote (Phase 3) has no
provider-removal scenario at all.

**Extending `platform/shell/src/coordinator.rs`** (this migration is what actually exercises the
generic `Screen`/`Overlay` split against LVGL's real failure modes, not a fake renderer): new
`queue_intent`/`pop_intent` (pure passthrough to `ShellModel`, for callers -- an LVGL event callback
still inside `lv_timer_handler` -- that cannot dispatch synchronously and must enqueue for a later
drain, matching `Backend`'s existing `drain_navigation` structure); `active_screen`/`active_screen_mut`/
`cleanup_blocked_screen`, `live_overlays`/`live_overlays_mut`, `overlay_cleanup_blocked` (read
iteration, for per-frame policy the coordinator doesn't itself decide -- Meditamer's Ambient-View
screen-exclusive overlay visibility sweep and its system-layer input-capture revocation, neither of
which the coordinator's own dispatch touches); `navigation_faulted`/`composition_faulted`/
`lifecycle_audit_faulted` getters and setters (the fixture's own audit needs to *set* a fault the
coordinator itself can't detect -- a callback-route purge count mismatch); `into_owned_instances`
(consumes the coordinator, returns every owned instance, for tearing a partially built coordinator back
down after bring-up fails partway through -- `Backend::destroy_initial_backend_or_stop`);
`dispatch_overlay_removal` (removes a specific live overlay by token regardless of whether it is the
active modal -- `dispatch_composition`'s `DismissActiveModal` only ever targets the current modal, and
cannot express Meditamer's sticky refresh-control overlay's toggle-off, which is not a modal at all).

**Two real, pre-existing-in-shape-but-newly-instantiated gaps in `coordinator.rs` itself, found and
fixed while wiring Meditamer, not introduced by it:**

- `bootstrap_screen`'s doc comment always claimed "enters, activates and enables" but its body only
  ever called `enter` -- latent since Phase 3, because Medinote's `activate`/`enable` are trivially
  `true`. Fixed by making the body match the doc (`activate` then `enable`, tearing the candidate back
  down and latching a fault via a new `cleanup_blocked_screen` retry path on either failing) and adding
  a typed `BootstrapError<EnterError>`. Meditamer's own bootstrap keeps its extra bring-up ceremony
  (swapping away LVGL's implicitly-created default screen before deleting it) hand-written rather than
  routed through the fixed `bootstrap_screen` -- that ceremony has no `SurfaceRuntime` equivalent -- via
  a new, narrower `install_bootstrap_screen(instance)` that just accepts an already-fully-prepared
  instance.
- `dispatch_navigation`'s `RolledBack` arm and `retry_blocked_cleanup` both discarded whether the
  transition's rollback had actually restored the origin (`RollbackReason`'s `origin_restored` flag),
  where `Backend::settle_rolled_back`/`Backend::retry_blocked_cleanup` (pre-migration) forced ambient
  Home recovery whenever it hadn't. Never reachable through Phase 2's fake-renderer tests or Phase 3's
  Medinote adapter (`activate`/`quiesce` can't fail there), so this was a real, latent gap, not a
  regression. Ported the exact logic into two new private coordinator methods --
  `settle_rolled_back_screen` (the origin-restored branch) and `recovery_transition` (`Backend::
  recover_ambient`'s body: always a fresh `ShellModel::prepare_recovery_home` -- never reuses the
  active instance token, since that may be exactly the broken one -- through a full `execute_transition`
  against a caller-supplied origin) -- shared by both call sites, exactly like the two `Backend` methods
  were. A new host test, `unrestored_origin_after_a_failed_activation_forces_ambient_recovery`, forces
  this exact sequence with a fake screen runtime that fails activation twice (candidate, then origin
  restore) and confirms the coordinator recovers to a fault-free Home rather than latching.

Host evidence (`cargo test -p shell --target aarch64-apple-darwin`, `bash scripts/host-test.sh lint
shell`/`test render`): 58 `shell` tests now pass (56 before this phase, plus
`unrestored_origin_after_a_failed_activation_forces_ambient_recovery` and
`dispatch_overlay_removal_removes_a_specific_non_modal_overlay_by_token`, the latter covering the new
`dispatch_overlay_removal` method against a bystander overlay that must stay live and an already-gone
token that must be rejected, not silently ignored). `render`'s existing `lvgl_adapter::tests::
safety_boundary` is unaffected (Phase 1's adapter itself did not change). Clippy (`-D warnings`) clean
on both crates. Both `targets/meditamer-inkplate` builds (`debug`/`release`, `default`/`all-features`)
and Clippy (`default`/`all-features`, `-D warnings`) pass with no new warnings; one pre-existing,
unrelated `dead_code` warning persists (same as E-0002). `targets/medinote-waveshare`'s `bash build.sh`
(release) and `cargo clippy -- -D warnings` also stay clean after the shared `coordinator.rs` changes.

Physical-device evidence, Inkplate `e8:6b:ea:fb:d5:54` (`/dev/cu.usbserial-8310`), via
`scripts/device/flash.sh debug` (each capture's own archived `firmware.elf`/`sha256.txt` cited below):

- **`ui-initialization-fixture` boot** (ELF `99b2e8d7e28fdae94b6e791fb31d271c9a29bd62e41ef0603cda63364f145200`,
  pre-fix code, and re-run against the final code at ELF
  `92b249c7c02f509ad0aa58865e09289208de738789a7ef22c32dbfcac19a2e13`): all 13 `UI_INIT_FIXTURE ...
  outcome=passed` markers passed both times -- the same 2-round/5-stage injected-failure sequence plus
  reinitialization E-0003 recorded pre-migration, now running through `install_bootstrap_screen` and
  `into_owned_instances`-based teardown instead of the old hand-rolled fields. No panic, no
  `integrity_ok=false`, in either capture.
- **`hostctl test ui-lifecycle`** (ELF `4292e2a4ea321b2695d791165500c62d189801a4cbc3b1eae589ef7e01bab006`,
  `ui-provider-fixture` build, pre-fix code -- this acceptance path never touches provider removal):
  `--cycles 30 --max-baseline-drift-bytes 256` failed ("LVGL high-water did not plateau over the final
  30 transitions"); inspecting the archived log showed `lvgl_used` cycling among the same three values
  every round (no growth), `heap_external_free`/`heap_external_min`/`heap_peak_used`/`cpu0_stack_min`
  bit-for-bit flat across all 90 transitions, and `lvgl_max_used` (LVGL's own high-water statistic)
  settling by transition 65 of 90 and never moving again -- an allocator warm-up tail longer than a
  30-cycle window, not a leak. Re-run at `--cycles 50` (matching the historical pre-migration baseline
  parameters) passed clean: `UI lifecycle passed: cycles=50 steps=150`, zero-tolerance 256-byte drift,
  150 real `UISTEP`-driven transitions through `dispatch_navigation`/`retry_blocked_cleanup`.
- **Provider-fixture removal, live `UIFIXTURE` serial commands** (two ELFs, see the bugs below): drove
  the full route by hand over a raw serial connection (`stty -f <fd> 115200 raw -echo` + `printf` into
  a persistently-held file descriptor, then `espflash monitor --non-interactive` to capture the
  response -- `espflash monitor`'s own interactive stdin forwarding does not work non-interactively in
  this harness, confirmed again this phase) -- Home -> Launcher -> ProviderFixture -> provider overlay
  modal -> Confirm preempts it -> `UI_PROVIDER_REMOVE state=staged` -> `state=detached
  callback_actions_purged=1` -> `state=finalized definitions=2 overlays=1 queued=1` -> fallback lands on
  Home -> the provider re-registers and a **second** full cycle (fresh `ProviderGeneration(2)`) repeats
  identically. No panic, no `integrity_ok=false`, no `navigation_faulted=true`, no
  `composition_faulted=true`, no `lifecycle_audit_faulted=true`, anywhere across both cycles' full
  serial logs.

**Two real bugs found and fixed during this hardware verification, not introduced by anything but this
migration's own reordering:**

1. **Logging only, no behavioral effect:** `UI_NAV state=committed ... role={:?}` captured `role` from
   `self.coordinator.shell().active().role` *before* dispatching, i.e. the origin's role, not the
   destination's -- found immediately when a `UISTEP`-driven Ambient View -> Launcher transition logged
   `role=Ambient` instead of `role=Launcher`. Fixed by reading `role` after the dispatch call, alongside
   where the destination surface was already read post-dispatch. Confirmed fixed at ELF
   `f21e8433b6754860ee789dc36cfc3eac802d8534bbbd5eadb78a47d64f2ab207`: three `UISTEP`s from Ambient View
   correctly logged `role=Launcher`, `role=SystemRoot`, `role=Ambient` in turn.
2. **Real, would-always-fail:** `execute_provider_removal_fixture`'s callback-purge audit
   (`intent_bridge::purge_provider(owner)`, expected to find and purge exactly the one still-queued
   "remove clicked" callback action) ran *after* `dispatch_provider_removal` returned `Detached`, but
   `dispatch_provider_removal`'s own origin teardown (`ActiveSurface::destroy`'s existing, unconditional
   `intent_bridge::purge_instance` call) had already purged that exact same queue entry by then -- it is
   sourced from the origin's own instance token. So `purged` was always `0`, never `1`, and the audit
   would fail on every run, not intermittently: confirmed live (`state=audit_failed stage=callback_purge
   ... expected=1 actual=0`) at ELF `4292e2a4ea321b2695d791165500c62d189801a4cbc3b1eae589ef7e01bab006`.
   The original hand-rolled version never had this problem because its purge ran *inside*
   `execute_transition`'s commit closure, strictly before the origin's teardown; `dispatch_provider_
   removal` commits atomically with no such mid-commit hook, so there is no point to run it at *after*
   dispatch starts. Fixed by purging first, immediately after the alignment check and before dispatching
   at all -- confirmed fixed (`callback_actions_purged=1`, `state=finalized`) at ELF
   `aa12e04883eeb067e9011242640b14964551092bc302365ba17ae5aecc13fb02`, across both full removal cycles
   above.

Limits: the on-device evidence never exercised the `origin_restored=false` forced-recovery path (the
new host test above is that path's only evidence) -- Meditamer's real `LvglSurfaceRuntime` has no
practical way to fail `activate`/`quiesce` in a live session to provoke it. `ui-lifecycle`'s 50-cycle
acceptance only drove `UISTEP` (Home/Launcher/Diagnostics), never a live overlay admission/removal or
the Settings-driven `dispatch_overlay_removal` path (no serial command reaches it; it needs a physical
tap through Overlay Settings, not attempted this phase). The `ble-release` profile and the
`all-features` build were checked at the host/Clippy level only, not flashed. No differential
memory/timing comparison against a pre-Phase-4 baseline was taken, for the same reason Phases 1 and 3
waived theirs -- the `ui-lifecycle` 50-cycle run's absolute numbers (zero drift at a 256-byte tolerance)
stand on their own. `products/meditamer`'s remaining screens/widgets (Launcher, Diagnostics,
AmbientView, OverlayToggles -- renamed from OverlaySettings in Phase 5, see E-0008, to stop colliding
with the new Settings overlay's name -- NavigationCue, Confirm, ProviderFixture) still use raw unsafe
LVGL internally, invoked through `SurfaceRuntime`/`CompositionRuntime` now instead of hand-called
`execute_transition`/manual staging -- extending `platform/render`'s safe adapter to them was explicitly
scoped out of this phase, as a separate, later body of work.

### E-0008 -- Phase 5: Settings overlay working on both products, physical confirmation pending

Both products now request a minimal Settings overlay -- a small panel proving the coordinator can hold
Home live underneath an admitted modal, the plan's Objective naming this the integration example --
over their own still-live Home. Per explicit user direction, this is a bare-minimum placeholder (a
title, one or two lines of text, and a dismiss path) rather than any real settings content; Meditamer's
existing `OverlayToggles` screen (renamed from `OverlaySettings` this phase specifically to stop
colliding with this new overlay's name -- same rename in `platform/shell::catalogue::CatalogueViewKind`,
since Medinote never used that variant, and every product-side reference) is untouched.

**Meditamer**: a new `SETTINGS_SURFACE_ID` (`SurfaceRole::Overlay`) and `SettingsPanel`
(`products/meditamer/src/firmware/ui/overlay/base_overlays.rs`) built exactly like the existing
`ConfirmModal` -- a modal `lv_layer_sys()` child claimed through `IntentBindings::Modal`, dismissed via
`CompositionIntent::DismissActiveModal` -- with a "Close" button reusing `ConfirmModal`'s own
`create_modal_button`/`create_label` helpers. `LvglOverlayRuntime::enter` gained one more surface-
identity branch. The trigger is a new "Settings" button on `HomeScreen` (top-left, `#![forbid(unsafe_code)]`,
built through the Phase 1 safe adapter like the rest of Home) wired to `intent_bridge::
show_confirm_callback` -- Home's own `show_confirm` modal-request slot, which nothing on Home used
before this (Home never showed Confirm itself), now requests `surfaces.settings` instead of
`surfaces.confirm` specifically for the Home screen instance (`ActiveSurface::enter`); every other
screen's own `show_confirm` binding is untouched. No new callback machinery was needed.

**Medinote**: Medinote has no touchscreen, so (explicit product direction: "invoke settings
programmatically") Settings is opened and closed over the same JTAG console byte stream every other
target command already uses, not a physical button gesture -- two new `ConsoleCommand` variants,
`UiSettings`/`UiClose` (`products/medinote/src/observations/fixture.rs`, literal `UISETTINGS`/`UICLOSE`
lines, same recognition shape as the existing `PING`), surfaced through `poll_observations`'s new third
return value (`SettingsOverlayRequest`) for `runtime_ui_task`'s main loop to dispatch. `MedinoteOverlay`
became a proper enum (`BleStatus`/`Settings`) instead of a single-purpose struct, matching Meditamer's
`ActiveOverlay`; `MedinoteOverlayRuntime::enter` infers overlay kind from `instance.token.surface`
identity the same way. New `SettingsWidget` (`targets/medinote-waveshare/src/ui/settings_widget.rs`)
follows the existing `BleStatusWidget`'s exact shape (a standalone `lv_layer_top()` child) but is purely
visual -- no interactive children, since dismissal is the `UICLOSE` command, not a tap.

**A real gap found and fixed while wiring this, not previously reachable**: Medinote's KEY/BOOT handling
branches directly on `coordinator.shell().active().role` every tick with no concept of input capture --
unlike Meditamer's touchscreen, which LVGL's own hit-testing already excludes from objects the system
layer's modal panel covers. Before this phase nothing on Medinote was ever modal, so this was never
exercised; with Settings possibly live, an unguarded BOOT press on Home would still fire deep-sleep (or
KEY would still open the Launcher) *through* the open Settings panel. Fixed with an explicit
`coordinator.shell().active_modal().is_some()` guard around the existing KEY/BOOT role-dispatch block
(the headless sleep-fixture path stays unconditional, matching its own existing "reaches Home's actions
independent of whatever else the UI is doing" precedent for `NavIntent::Home`) -- this runtime loop's own
equivalent of Meditamer's LVGL-level exclusive input capture, since Medinote has no LVGL click-target
concept for its physical buttons to go through.

**Also fixed in passing**: `#[cfg(not(feature = "cheertok-controls"))] struct MedinoteOverlay` referenced
`OverlayInstance` without importing it (only imported under `#[cfg(feature = "cheertok-controls")]`,
which is Medinote's own default feature) -- a latent, pre-existing break in the never-built,
non-default `--no-default-features` configuration, unrelated to Phase 5's own design. Restructuring
`MedinoteOverlay` into an enum moved `OverlayInstance` (with `OverlayInput`/`OverlayLifetime`, needed by
Settings unconditionally either way) to the crate's unconditional import block, fixing it as a side
effect; confirmed by a clean `cargo clippy --no-default-features -- -D warnings` this phase, the first
time that configuration has been checked in this ledger.

Host evidence: `products/medinote` gained one test, `console_recognizes_settings_open_and_close_commands`
(`UISETTINGS`/`UICLOSE`, both line-ending styles) -- 116 `products/medinote` tests pass (115 before this
phase), `cargo test -p shell` stays at 58 (untouched this phase). `targets/meditamer-inkplate`: `debug`/
`release`, `default`/`all-features` builds and Clippy (`-D warnings`) all pass, no new warnings.
`targets/medinote-waveshare`: `bash build.sh` (release, default features) and Clippy with *both*
`cheertok-controls` on (default) and `--no-default-features` (see the fix above) pass clean.

Limits: **no physical-device evidence this entry** -- the user was away from both boards when this
phase's implementation finished ("let's implement what we can and leave human verification til the
end"); by design and by the same mechanisms Confirm/CheerTok already prove live (Modal admission,
`DismissActiveModal`, `lv_layer_sys()`/`lv_layer_top()` overlay construction), Settings is expected to
work the same way. Still to confirm live, not yet demonstrated: Meditamer's new Settings button
responding to a real touch, Home staying visibly alive and its ambient content still updating behind the
panel, the dismiss button's focus-restoration behavior, Medinote's `UISETTINGS`/`UICLOSE` commands
reaching the coordinator over real serial, and the new BOOT/KEY modal guard actually preventing a
sleep-through-Settings on real hardware -- close this the same way Phase 1/3/4's own live sessions did,
next time at the hardware. "Save" and "cleanup failures" (from the plan's Phase 5
bullets) were not exercised either: the placeholder overlay has no state to save, and neither product's
copy has a way to force a live cleanup fault without hardware to observe it on. Meditamer's Settings
button was not tested against `ui-provider-fixture`/`ui-initialization-fixture` interaction (the modal
request slot Home now uses for Settings is shared machinery, but the fixtures themselves don't reach
Home's own button).

### E-0009 -- Phase 6: background-update requirements already held by construction; one prerequisite fixed

Phase 6's scope was narrowed to clock/sensor/service-status delivery
reaching Home while an overlay is live, late replies handled by identity, and fixture operations. Reading
the reused infrastructure closely (`environment_delivery.rs`/`battery_delivery.rs` on Meditamer,
`observations.rs`/`runtime_ui.rs` on Medinote) against each of those found no gap needing new delivery
code -- every one already holds, by construction, from pre-ADR-0017 typed-observation machinery ("Typed
observations are complete," predating this plan) plus Phase 2's own overlay/screen separation:

- Delivery ownership (`Backend::committed_environment_owner` on Meditamer; `coordinator.shell().active().role`
  gates on Medinote) reads only which *screen* is active, never live-overlay state -- an admitted Settings
  panel changes neither, so updates were never at risk of stopping because of it.
- `EnvironmentDeliveryState::take_delivery` (and battery's equivalent) already rejects any provider reply
  whose owner token doesn't match the currently reconciled owner, and `reconcile` mints a fresh
  `OwnerGeneration` on every owner change specifically to keep a stale in-flight request from being
  mistaken for a fresh one -- unchanged by anything in this plan, already proven by Phase 4/E-0056.
- Fixture routing (`service_fixture`/`poll_fixture` on Meditamer, `ConsoleCommand::Fixture` on Medinote)
  is already explicitly independent of which surface currently owns live delivery ("The fixture is
  serviced even without an Ambient Home owner").
- Neither product has any feature where a clock/sensor/service-status update itself triggers navigation
  or composition (checked this phase: no low-battery warning overlay, no auto-navigation on a sensor
  threshold) -- "route updates through typed adapters where they change navigation/composition" is
  vacuously satisfied because nothing currently does that.
- Coalescing/capacity: environment/battery/clock delivery runs over its own dedicated channels, entirely
  separate from `intent_bridge`'s and `ShellModel`'s own bounded queues that lifecycle/failure events
  travel over -- there is no shared capacity for a slow sensor update to exhaust and starve a modal
  dismissal or cleanup retry with.

**The one real gap this phase found**: Meditamer's environment/battery delivery is keyed to `AmbientView`
(`SurfaceModel::AmbientView`, id 7), not the literal `Home` screen (id 1) Phase 5 first wired Settings to
-- so "updates keep reaching Home while Settings is active" had no real screen to be tested against on
Meditamer as Phase 5 left it. Per explicit user direction, fixed by also routing Ambient View's own
`show_confirm` modal-request slot to Settings: a second on-screen "Settings" button
(`products/meditamer/src/firmware/ui/screen/ambient_view/mod.rs`, raw LVGL matching the rest of that
screen -- unlike Home's, which goes through the Phase 1 safe adapter), visible in both `Mode::Ambient` and
`Mode::TimeDisplay` (unlike the Back button, which rides with the time display only). `ActiveSurface::enter`'s
`requested_overlay` resolution now checks `frame.surface == surfaces.home || frame.surface ==
surfaces.ambient_view`. Medinote needed no equivalent fix: its "Home" (`SurfaceRole::Ambient`) already is
the one screen carrying both the clock and (this phase's) Settings trigger, so this gap only existed on
Meditamer's side, where Home and the real ambient/clock screen are two different surfaces.

Host evidence: none possible for the delivery-logic reasoning above -- `cargo check -p meditamer-product
--target aarch64-apple-darwin` fails building `esp-hal`/`esp32` themselves (unconditional dependencies of
the whole crate, unrelated to which module is under test), confirming `products/meditamer` has no
host-testable target at all, so `EnvironmentDeliveryState`/`BatteryDeliveryState`'s pure-Rust logic
cannot get new host coverage despite not itself depending on LVGL or xtensa. `targets/meditamer-inkplate`:
`debug`/`release`, `default`/`all-features` builds and Clippy (`-D warnings`) all pass, no new warnings,
after the Ambient View button addition.

Limits: no physical-device evidence this entry either, for the same reason as E-0008 -- folded into the
same pending session rather than a separate one (see the plan's "Next action"). Nothing here has been
confirmed live: not the new Ambient View Settings button rendering or responding to a real touch, not
environment/clock content actually continuing to update behind an open Settings panel on real hardware
on either product, not a live fixture command arriving while Settings is open. "Keep input, cleanup and
modal handling responsive during slow sensor, storage, radio and persistence work" was reasoned about
(the delivery channels are structurally separate from the queues those need) but not independently
re-verified -- the display task's own async scheduling predates ADR-0017 entirely and nothing in Phases
1-6 touched it.

### E-0010 -- Local input work item 2: WAKE routing confirmed, one real cancellation gap found and fixed

Local buttons and touch's [Remaining work item 2](local-buttons-and-touch.md#remaining-work) covers
routing accepted Inkplate WAKE edges through product policy, preserving the frontlight action and joint
touch classification, and handling acquisition fault/suspension cancellation. Reading the pipeline
(`products/meditamer/src/firmware/input/gpio36.rs`, `.../display/gpio36_feedback.rs`, and the touch
acquisition task) against each requirement found routing, frontlight preservation and joint
classification already correct by construction: `Gpio36Classifier` emits `WakeButtonPressed`/`Released`
independently of touch contact state, `publish_action` forwards only those two variants to
`AppEvent::Gpio36Action`, and `gpio36_feedback::handle_gpio36_action` cycles the frontlight on press
without going through `intent_bridge` or any modal-capture check -- matching the shared input contract's
explicit carve-out for global actions ("hardware execution remains with the existing resource owner").

**The one real gap this item found**: `suspend_touch_acquisition`'s handling already called
`classifier.cancel_pending()` before quiescing (and `run_pipeline_replay_probe` does the same before a
replay), but `handle_fault` (`.../touch/tasks/acquisition/runtime.rs`) did not -- a touch-acquisition
fault could leave a WAKE/touch classification pending through shutdown and the retry backoff, so recovery
could resolve it against elapsed downtime that has nothing to do with the original GPIO36 assertion.
Fixed by adding a `classifier: &mut Gpio36Classifier` parameter to `handle_fault` and calling
`cancel_pending()` inside it, matching the suspend path exactly; updated all five call sites (main poll
loop's recovery branch, the asserted-line probe's fault branch, and both `apply_resume` fault branches).

Host evidence: none possible, for the same reason as E-0009 -- `products/meditamer` has no host-testable
target. `targets/meditamer-inkplate` Clippy (`-D warnings`), `default` and `all-features` profiles, both
pass clean beyond the one pre-existing, unrelated `wifi_ipv4` dead-code warning also seen in earlier
entries.

Limits: no physical-device evidence -- the fault path's corrected cancellation has not been exercised
against a real touch-controller fault or a real WAKE press on hardware; folded into the same pending
physical-confirmation session as E-0008/E-0009. Remaining work item 3 (touch delivery under the shared
ownership rules) is unstarted.

### E-0011 -- Local input work item 3: Down/Up-only delivery confirmed, one real capture-entry gap found and fixed

Local buttons and touch's [Remaining work item 3](local-buttons-and-touch.md#remaining-work) covers
adapting Inkplate touch delivery to the shared ownership rules: Down/Up-only consumption without
duplicate tap actions, preserving the production LVGL and multitouch paths. Reading the pipeline
(`products/meditamer/src/firmware/touch/core/engine.rs`, `.../ui/lvgl/io.rs`, `.../ui/lvgl/backend/
frame.rs`, `.../display/presentation.rs`) against each requirement found Down/Up-only delivery already
correct by construction: `io::update_touch` mutates the raw pressed/x/y state LVGL's indev callback
reads only on `Down`/`Move`/`LongPress`/`Up`/`Cancel`; the touch engine's own `Tap`/`Swipe` recognitions
are dead-ended at that same match (used only for debug logging in `touch/debug_log.rs`, confirmed by
grep across every other consumer), so they never re-deliver a second press/release pulse into LVGL and
cannot produce a duplicate click alongside the genuine Down/Up pair. Multitouch shares the same single
pointer indev via a separately queued batch read (`read_multitouch`), not a second registered indev.
LVGL's own native gesture detector (pinch/rotate/two-finger-swipe -- a different mechanism from the
touch engine's recognizer) was already confirmed gated behind `active_modal().is_some()` in
`show_gesture` (`backend/frame.rs`).

**The one real gap this item found**: exclusive system-layer capture
(`LvglOverlayRuntime::set_exclusive_capture` in `ui/lvgl/backend.rs`, the `CompositionRuntime` method
`UiCoordinator` calls on every modal's entry) only toggled the top layer's `LV_OBJ_FLAG_CLICKABLE` flag.
That flag changes LVGL's hit-testing for a *new* press -- any press starting after capture is granted
hits the top layer first, by z-order, so the underlying screen never sees it -- but LVGL does not
re-hit-test a press already in flight; it keeps delivering PRESSING/RELEASED to whatever object it
originally found regardless of what gets added above it afterward. A modal opened by anything other
than the held widget's own completed click -- BOOT/KEY, a Medinote-style console command, a second
finger already down when a first finger's tap opens the modal -- could let that stale press's eventual
release fire a real click on a screen the modal now visually covers: exactly the shared input contract's
"a new surface must not inherit an old hold or click," on the touch side rather than the button
`CaptureGate` side E-0010's predecessor items already hold. Fixed by calling `lv_indev_reset(indev, NULL)`
on the pointer indev at the same point capture is granted (guarded on the indev pointer being non-null,
since a null `indev` argument resets *every* registered indev per LVGL's own semantics, not the one
intended) -- canceling any in-flight press before the modal claims input, without synthesizing a release
event that would itself fire a phantom click on the object being cancelled.

Host evidence: none possible, for the same reason as E-0009/E-0010 -- `products/meditamer` has no
host-testable target. `targets/meditamer-inkplate` Clippy (`-D warnings`), `default` and `all-features`
profiles, both pass clean beyond the one pre-existing, unrelated `wifi_ipv4` dead-code warning. Unlike
E-0010, a full `release` build (not just Clippy) was also run to confirm the new `lv_indev_reset` FFI
call actually links against the bindgen-generated signature in the linked LVGL library, since Clippy
alone does not exercise the linker.

Limits: no physical-device evidence -- the corrected capture-entry reset has not been exercised against
a real held finger and a real modal-opening trigger on hardware; folded into the same pending
physical-confirmation session as E-0008/E-0009/E-0010. This closes local-buttons-and-touch.md's three
Remaining work items at the host/build-verification level; only Acceptance's own physical-device and
end-to-end checks (and the separate capture/replay plan's recognition-level work) remain on that plan.
