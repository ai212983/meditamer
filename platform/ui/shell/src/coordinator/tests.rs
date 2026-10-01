use super::*;
use crate::lifecycle::LifecycleEvent;
use crate::model::ProviderRuntimeAudit;
use crate::navigator::NavigationFrame;
use crate::types::{
    CompositionIntent, InstanceGeneration, OverlayInput, OverlayLifetime, OwnedNavIntent,
    ProviderId, SurfaceCapabilities, SurfaceRef, SurfaceRole,
};

const BASE_PROVIDER: ProviderId = ProviderId(1);
const HOME: u16 = 1;
const LAUNCHER: u16 = 2;
const APP_ROOT: u16 = 3;
const APP_CHILD: u16 = 4;
const SETTINGS_ROOT: u16 = 5;
const PASSIVE_OVERLAY: u16 = 10;
const INTERACTIVE_OVERLAY: u16 = 11;
const MODAL_A: u16 = 12;
const MODAL_B: u16 = 13;

fn base_specs() -> [SurfaceSpec; 9] {
    [
        SurfaceSpec::new(
            HOME,
            SurfaceRole::Ambient,
            SurfaceCapabilities::AMBIENT,
            RefreshHint::Boundary,
        ),
        SurfaceSpec::new(
            LAUNCHER,
            SurfaceRole::Launcher,
            SurfaceCapabilities::NONE,
            RefreshHint::Boundary,
        ),
        SurfaceSpec::new(
            APP_ROOT,
            SurfaceRole::AppRoot,
            SurfaceCapabilities::LAUNCHABLE,
            RefreshHint::Boundary,
        ),
        SurfaceSpec::new(
            APP_CHILD,
            SurfaceRole::AppChild,
            SurfaceCapabilities::NONE,
            RefreshHint::Content,
        ),
        SurfaceSpec::new(
            SETTINGS_ROOT,
            SurfaceRole::SystemRoot,
            SurfaceCapabilities::LAUNCHABLE,
            RefreshHint::Boundary,
        ),
        SurfaceSpec::new(
            PASSIVE_OVERLAY,
            SurfaceRole::Overlay,
            SurfaceCapabilities::OVERLAY,
            RefreshHint::Micro,
        ),
        SurfaceSpec::new(
            INTERACTIVE_OVERLAY,
            SurfaceRole::Overlay,
            SurfaceCapabilities::OVERLAY,
            RefreshHint::Micro,
        ),
        SurfaceSpec::new(
            MODAL_A,
            SurfaceRole::Overlay,
            SurfaceCapabilities::OVERLAY,
            RefreshHint::Content,
        ),
        SurfaceSpec::new(
            MODAL_B,
            SurfaceRole::Overlay,
            SurfaceCapabilities::OVERLAY,
            RefreshHint::Content,
        ),
    ]
}

#[derive(Debug, Eq, PartialEq)]
struct FakeScreen {
    surface: SurfaceRef,
}

#[derive(Default)]
struct FakeScreenRuntime {
    enter_ok: bool,
    activate_ok: bool,
    quiesce_ok: bool,
    enable_ok: bool,
    destroy_ok: bool,
    destroy_audit_failed: bool,
    // Forces the next N `activate` calls to fail regardless of
    // `activate_ok`, decrementing on each -- for provoking a specific
    // *sequence* of activation outcomes (candidate fails, origin-restore
    // also fails, a later recovery attempt succeeds) that a single
    // steady-state flag can't express.
    activate_failures_remaining: u32,
    live: u32,
    events: Vec<LifecycleEvent, 16>,
}

impl FakeScreenRuntime {
    fn healthy() -> Self {
        Self {
            enter_ok: true,
            activate_ok: true,
            quiesce_ok: true,
            enable_ok: true,
            destroy_ok: true,
            destroy_audit_failed: false,
            activate_failures_remaining: 0,
            live: 0,
            events: Vec::new(),
        }
    }
}

impl SurfaceRuntime for FakeScreenRuntime {
    type Instance = FakeScreen;
    type EnterError = &'static str;

    fn enter(
        &mut self,
        frame: NavigationFrame,
        _token: SurfaceInstanceToken,
    ) -> Result<Self::Instance, Self::EnterError> {
        if !self.enter_ok {
            return Err("enter");
        }
        self.live += 1;
        Ok(FakeScreen {
            surface: frame.surface,
        })
    }

    fn activate(&mut self, _instance: &Self::Instance) -> bool {
        if self.activate_failures_remaining > 0 {
            self.activate_failures_remaining -= 1;
            return false;
        }
        self.activate_ok
    }

    fn quiesce(&mut self, _instance: &Self::Instance) -> bool {
        self.quiesce_ok
    }

    fn enable(&mut self, _instance: &Self::Instance) -> bool {
        self.enable_ok
    }

    fn destroy(&mut self, instance: Self::Instance) -> Result<(), DestroyFailure<Self::Instance>> {
        if !self.destroy_ok {
            return Err(DestroyFailure::Live(instance));
        }
        self.live -= 1;
        if self.destroy_audit_failed {
            return Err(DestroyFailure::Audit);
        }
        Ok(())
    }

    fn observe(&mut self, event: LifecycleEvent) {
        let _ = self.events.push(event);
    }
}

#[derive(Debug, Eq, PartialEq)]
struct FakeOverlay {
    instance: OverlayInstance,
}

struct FakeOverlayRuntime {
    enter_ok: bool,
    enable_ok: bool,
    disable_ok: bool,
    destroy_ok: bool,
    destroy_audit_failed: bool,
    live: u32,
    exclusive_capture: bool,
}

impl FakeOverlayRuntime {
    fn healthy() -> Self {
        Self {
            enter_ok: true,
            enable_ok: true,
            disable_ok: true,
            destroy_ok: true,
            destroy_audit_failed: false,
            live: 0,
            exclusive_capture: false,
        }
    }
}

impl CompositionRuntime for FakeOverlayRuntime {
    type Instance = FakeOverlay;
    type EnterError = &'static str;

    fn enter(&mut self, instance: OverlayInstance) -> Result<Self::Instance, Self::EnterError> {
        if !self.enter_ok {
            return Err("enter");
        }
        self.live += 1;
        Ok(FakeOverlay { instance })
    }

    fn instance(&self, live: &Self::Instance) -> OverlayInstance {
        live.instance
    }

    fn show(&mut self, _live: &Self::Instance) {}

    fn hide(&mut self, _live: &Self::Instance) {}

    fn enable(&mut self, _live: &Self::Instance) -> bool {
        self.enable_ok
    }

    fn disable(&mut self, _live: &Self::Instance) -> bool {
        self.disable_ok
    }

    fn destroy(&mut self, live: Self::Instance) -> Result<(), DestroyFailure<Self::Instance>> {
        if !self.destroy_ok {
            return Err(DestroyFailure::Live(live));
        }
        self.live -= 1;
        if self.destroy_audit_failed {
            return Err(DestroyFailure::Audit);
        }
        Ok(())
    }

    fn set_exclusive_capture(&mut self, enabled: bool) {
        self.exclusive_capture = enabled;
    }
}

type TestCoordinator = UiCoordinator<FakeScreen, FakeOverlay, 8, 20, 8, 4, 4, 8>;

fn coordinator() -> (TestCoordinator, FakeScreenRuntime, FakeOverlayRuntime) {
    let mut coordinator =
        TestCoordinator::new(BASE_PROVIDER, &base_specs(), SurfaceId(HOME)).unwrap();
    let mut screens = FakeScreenRuntime::healthy();
    let overlays = FakeOverlayRuntime::healthy();
    coordinator.bootstrap_screen(&mut screens).unwrap();
    assert!(coordinator.active_screen_ready());
    (coordinator, screens, overlays)
}

fn base_owner(coordinator: &TestCoordinator) -> ProviderToken {
    coordinator.shell().active().surface.owner
}

fn surface(owner: ProviderToken, id: u16) -> SurfaceRef {
    SurfaceRef::new(owner, id)
}

fn navigate(
    coordinator: &mut TestCoordinator,
    intent: NavIntent,
    screens: &mut FakeScreenRuntime,
    overlays: &mut FakeOverlayRuntime,
) -> NavigationDispatchOutcome {
    coordinator
        .dispatch_navigation(intent, screens, overlays)
        .expect("dispatch is not faulted in these tests")
}

#[test]
fn navigation_depths_push_and_pop_through_launcher_root_and_child() {
    let (mut coordinator, mut screens, mut overlays) = coordinator();
    let owner = base_owner(&coordinator);

    assert_eq!(
        navigate(
            &mut coordinator,
            NavIntent::OpenLauncher(surface(owner, LAUNCHER)),
            &mut screens,
            &mut overlays
        ),
        NavigationDispatchOutcome::Committed { changed: true }
    );
    assert_eq!(
        coordinator.shell().active().surface,
        surface(owner, LAUNCHER)
    );

    assert_eq!(
        navigate(
            &mut coordinator,
            NavIntent::Launch(surface(owner, APP_ROOT)),
            &mut screens,
            &mut overlays
        ),
        NavigationDispatchOutcome::Committed { changed: true }
    );
    assert_eq!(
        coordinator.shell().active().surface,
        surface(owner, APP_ROOT)
    );

    assert_eq!(
        navigate(
            &mut coordinator,
            NavIntent::Push(surface(owner, APP_CHILD)),
            &mut screens,
            &mut overlays
        ),
        NavigationDispatchOutcome::Committed { changed: true }
    );
    assert_eq!(
        coordinator.shell().active().surface,
        surface(owner, APP_CHILD)
    );
    assert_eq!(coordinator.shell().navigation_len(), 4);

    assert_eq!(
        navigate(
            &mut coordinator,
            NavIntent::Back,
            &mut screens,
            &mut overlays
        ),
        NavigationDispatchOutcome::Committed { changed: true }
    );
    assert_eq!(
        coordinator.shell().active().surface,
        surface(owner, APP_ROOT)
    );

    assert_eq!(
        navigate(
            &mut coordinator,
            NavIntent::Home,
            &mut screens,
            &mut overlays
        ),
        NavigationDispatchOutcome::Committed { changed: true }
    );
    assert_eq!(coordinator.shell().active().surface, surface(owner, HOME));
    assert_eq!(screens.live, 1);
}

#[test]
fn unrestored_origin_after_a_failed_activation_forces_ambient_recovery() {
    // A candidate's `activate` failing rolls back to the origin *if*
    // re-activating the origin itself succeeds -- but here it doesn't
    // either (two forced failures: the candidate's own activate, then
    // the origin-restore attempt), so `RollbackReason::Activation
    // {origin_restored: false}` should force `UiCoordinator` into its
    // own ambient-Home recovery rather than reporting the origin restored.
    let (mut coordinator, mut screens, mut overlays) = coordinator();
    let owner = base_owner(&coordinator);

    screens.activate_failures_remaining = 2;
    let outcome = navigate(
        &mut coordinator,
        NavIntent::OpenLauncher(surface(owner, LAUNCHER)),
        &mut screens,
        &mut overlays,
    );
    assert_eq!(outcome, NavigationDispatchOutcome::RolledBack);

    // The forced recovery's own fresh Home candidate is the third
    // `activate` call -- past the two forced failures -- so it
    // succeeds, and the coordinator ends up healthy on Home, not
    // latched at fault.
    assert!(!coordinator.navigation_faulted());
    assert!(!coordinator.is_faulted());
    assert!(coordinator.active_screen_ready());
    assert_eq!(coordinator.shell().active().surface, surface(owner, HOME));
}

#[test]
fn empty_and_populated_overlay_admission_are_both_clean() {
    let (mut coordinator, _screens, mut overlays) = coordinator();
    let owner = base_owner(&coordinator);
    assert_eq!(coordinator.live_overlay_len(), 0);

    let source = coordinator.shell().active_instance();
    let outcome = coordinator
        .dispatch_composition(
            OwnedCompositionIntent {
                source,
                intent: CompositionIntent::Request {
                    surface: surface(owner, PASSIVE_OVERLAY),
                    input: OverlayInput::Passive,
                    lifetime: OverlayLifetime::Sticky,
                    rank: 1,
                },
            },
            &mut overlays,
        )
        .unwrap();
    assert!(matches!(
        outcome,
        CompositionDispatchOutcome::Admitted(OverlayAdmission::Active(_))
    ));
    assert_eq!(coordinator.live_overlay_len(), 1);
    assert_eq!(overlays.live, 1);

    // Per-frame content updates (a status label's text, say) reach the
    // live instance without going through a dispatch -- and a token that
    // never admitted, or already left, finds nothing.
    let CompositionDispatchOutcome::Admitted(OverlayAdmission::Active(admitted)) = outcome else {
        unreachable!("checked above");
    };
    assert!(coordinator
        .live_overlay(admitted.token, &overlays)
        .is_some());
    assert!(coordinator
        .live_overlay_mut(admitted.token, &overlays)
        .is_some());
    assert!(coordinator.live_overlay(source, &overlays).is_none());
}

#[test]
fn passive_and_interactive_overlays_both_admit_live() {
    let (mut coordinator, _screens, mut overlays) = coordinator();
    let owner = base_owner(&coordinator);
    for (surface_id, input) in [
        (PASSIVE_OVERLAY, OverlayInput::Passive),
        (INTERACTIVE_OVERLAY, OverlayInput::Interactive),
    ] {
        let source = coordinator.shell().active_instance();
        let outcome = coordinator
            .dispatch_composition(
                OwnedCompositionIntent {
                    source,
                    intent: CompositionIntent::Request {
                        surface: surface(owner, surface_id),
                        input,
                        lifetime: OverlayLifetime::Sticky,
                        rank: 1,
                    },
                },
                &mut overlays,
            )
            .unwrap();
        assert!(matches!(
            outcome,
            CompositionDispatchOutcome::Admitted(OverlayAdmission::Active(_))
        ));
    }
    assert_eq!(coordinator.live_overlay_len(), 2);
}

#[test]
fn dispatch_overlay_removal_removes_a_specific_non_modal_overlay_by_token() {
    // Remove one live, non-modal overlay directly by its own token,
    // regardless of which surface is active or whether anything is the
    // active modal -- `dispatch_composition`'s `DismissActiveModal` only
    // ever targets the current modal and cannot express this.
    let (mut coordinator, _screens, mut overlays) = coordinator();
    let owner = base_owner(&coordinator);
    let source = coordinator.shell().active_instance();

    // A second, unrelated interactive overlay stays live throughout --
    // proving removal is exact, not "clear everything."
    let bystander = coordinator
        .dispatch_composition(
            OwnedCompositionIntent {
                source,
                intent: CompositionIntent::Request {
                    surface: surface(owner, INTERACTIVE_OVERLAY),
                    input: OverlayInput::Interactive,
                    lifetime: OverlayLifetime::Sticky,
                    rank: 1,
                },
            },
            &mut overlays,
        )
        .unwrap();
    let CompositionDispatchOutcome::Admitted(OverlayAdmission::Active(bystander)) = bystander
    else {
        unreachable!("checked by passive_and_interactive_overlays_both_admit_live");
    };

    let target = coordinator
        .dispatch_composition(
            OwnedCompositionIntent {
                source,
                intent: CompositionIntent::Request {
                    surface: surface(owner, PASSIVE_OVERLAY),
                    input: OverlayInput::Passive,
                    lifetime: OverlayLifetime::Sticky,
                    rank: 2,
                },
            },
            &mut overlays,
        )
        .unwrap();
    let CompositionDispatchOutcome::Admitted(OverlayAdmission::Active(target)) = target else {
        unreachable!("checked by passive_and_interactive_overlays_both_admit_live");
    };
    assert_eq!(coordinator.live_overlay_len(), 2);
    assert_eq!(overlays.live, 2);

    let outcome = coordinator
        .dispatch_overlay_removal(target.token, &mut overlays)
        .unwrap();
    assert_eq!(outcome, CompositionDispatchOutcome::Removed);
    assert_eq!(coordinator.live_overlay_len(), 1);
    assert_eq!(overlays.live, 1);
    assert!(coordinator.live_overlay(target.token, &overlays).is_none());
    assert!(coordinator
        .live_overlay(bystander.token, &overlays)
        .is_some());

    // An unknown/already-gone token is rejected, not silently a no-op --
    // matches `ShellModel::prepare_overlay_removal`'s own contract.
    assert!(matches!(
        coordinator.dispatch_overlay_removal(target.token, &mut overlays),
        Err(DispatchRejected::Composition(_))
    ));
}

#[test]
fn active_modal_admits_live_second_modal_queues_dismiss_promotes_it() {
    let (mut coordinator, _screens, mut overlays) = coordinator();
    let owner = base_owner(&coordinator);

    let source = coordinator.shell().active_instance();
    let first = coordinator
        .dispatch_composition(
            OwnedCompositionIntent {
                source,
                intent: CompositionIntent::Request {
                    surface: surface(owner, MODAL_A),
                    input: OverlayInput::Modal,
                    lifetime: OverlayLifetime::Sticky,
                    rank: 1,
                },
            },
            &mut overlays,
        )
        .unwrap();
    let CompositionDispatchOutcome::Admitted(OverlayAdmission::Active(first_instance)) = first
    else {
        panic!("first modal must admit active");
    };
    assert_eq!(coordinator.live_overlay_len(), 1);
    assert_eq!(coordinator.focus(), first_instance.token);

    let second = coordinator
        .dispatch_composition(
            OwnedCompositionIntent {
                source,
                intent: CompositionIntent::Request {
                    surface: surface(owner, MODAL_B),
                    input: OverlayInput::Modal,
                    lifetime: OverlayLifetime::Sticky,
                    rank: 1,
                },
            },
            &mut overlays,
        )
        .unwrap();
    let CompositionDispatchOutcome::Admitted(OverlayAdmission::Queued(second_instance)) = second
    else {
        panic!("second modal must queue");
    };
    // Queued admission needs no target effect: still exactly one live overlay.
    assert_eq!(coordinator.live_overlay_len(), 1);
    assert_eq!(overlays.live, 1);
    assert_eq!(coordinator.focus(), first_instance.token);

    let dismiss = coordinator
        .dispatch_composition(
            OwnedCompositionIntent {
                source: first_instance.token,
                intent: CompositionIntent::DismissActiveModal,
            },
            &mut overlays,
        )
        .unwrap();
    assert_eq!(dismiss, CompositionDispatchOutcome::Removed);
    // B was promoted into the live slot A vacated: still exactly one live
    // overlay, and the runtime saw a fresh `enter` for it.
    assert_eq!(coordinator.live_overlay_len(), 1);
    assert_eq!(overlays.live, 1);
    assert_eq!(coordinator.focus(), second_instance.token);

    let dismiss = coordinator
        .dispatch_composition(
            OwnedCompositionIntent {
                source: second_instance.token,
                intent: CompositionIntent::DismissActiveModal,
            },
            &mut overlays,
        )
        .unwrap();
    assert_eq!(dismiss, CompositionDispatchOutcome::Removed);
    assert_eq!(coordinator.focus(), source);
}

#[test]
fn provider_removal_falls_back_home_and_finalizes() {
    let (mut coordinator, mut screens, mut overlays) = coordinator();
    let base = base_owner(&coordinator);
    let fixture_owner = coordinator
        .register_provider(
            ProviderId(2),
            &[SurfaceSpec::new(
                101,
                SurfaceRole::AppRoot,
                SurfaceCapabilities::LAUNCHABLE,
                RefreshHint::Boundary,
            )],
        )
        .unwrap();

    navigate(
        &mut coordinator,
        NavIntent::OpenLauncher(surface(base, LAUNCHER)),
        &mut screens,
        &mut overlays,
    );
    navigate(
        &mut coordinator,
        NavIntent::Launch(surface(fixture_owner, 101)),
        &mut screens,
        &mut overlays,
    );
    assert_eq!(
        coordinator.shell().active().surface,
        surface(fixture_owner, 101)
    );

    screens.events.clear();
    let outcome = coordinator
        .dispatch_provider_removal(fixture_owner, &mut screens, &mut overlays)
        .unwrap();
    let ProviderRemovalDispatchOutcome::Detached(pending) = outcome else {
        panic!(
            "provider removal must detach: {outcome:?}",
            outcome = matches!(outcome, ProviderRemovalDispatchOutcome::Detached(_))
        );
    };
    // Detaching only removes references; navigation already fell back home.
    assert_eq!(coordinator.shell().active().surface, surface(base, HOME));
    assert_eq!(coordinator.focus(), coordinator.shell().active_instance());
    assert_eq!(
        screens.events.as_slice(),
        &[
            LifecycleEvent::CandidateEntered,
            LifecycleEvent::CandidateActivated,
            LifecycleEvent::OriginQuiesced,
            LifecycleEvent::CandidateEnabled,
            LifecycleEvent::ShellCommitted,
            LifecycleEvent::OriginDestroyed,
        ][..]
    );

    coordinator
        .finalize_provider_removal(&pending, ProviderRuntimeAudit::verified(fixture_owner))
        .unwrap();
}

#[test]
fn provider_removal_retains_failed_origin_cleanup_until_retry() {
    let (mut coordinator, mut screens, mut overlays) = coordinator();
    let base = base_owner(&coordinator);
    let fixture_owner = coordinator
        .register_provider(
            ProviderId(2),
            &[SurfaceSpec::new(
                101,
                SurfaceRole::AppRoot,
                SurfaceCapabilities::LAUNCHABLE,
                RefreshHint::Boundary,
            )],
        )
        .unwrap();

    navigate(
        &mut coordinator,
        NavIntent::OpenLauncher(surface(base, LAUNCHER)),
        &mut screens,
        &mut overlays,
    );
    navigate(
        &mut coordinator,
        NavIntent::Launch(surface(fixture_owner, 101)),
        &mut screens,
        &mut overlays,
    );

    screens.destroy_ok = false;
    let outcome = coordinator
        .dispatch_provider_removal(fixture_owner, &mut screens, &mut overlays)
        .unwrap();
    let ProviderRemovalDispatchOutcome::Detached(pending) = outcome else {
        panic!("provider removal must commit before origin cleanup");
    };
    assert_eq!(coordinator.shell().active().surface, surface(base, HOME));
    assert_eq!(coordinator.focus(), coordinator.shell().active_instance());
    assert!(coordinator.cleanup_blocked_screen().is_some());
    assert!(coordinator.is_faulted());

    screens.destroy_ok = true;
    coordinator.retry_blocked_cleanup(&mut screens, &mut overlays);
    assert!(!coordinator.is_faulted());
    coordinator
        .finalize_provider_removal(&pending, ProviderRuntimeAudit::verified(fixture_owner))
        .unwrap();
}

#[test]
fn provider_removal_candidate_rollback_preserves_an_audit_failure() {
    // Provider removal's fallback-candidate rollback must handle an audit
    // failure the same way navigation's own rollback and `bootstrap_screen`
    // already do: fault the coordinator and set `lifecycle_audit_faulted`,
    // rather than only recognizing `DestroyFailure::Live` and losing the
    // audit outcome.
    let (mut coordinator, mut screens, mut overlays) = coordinator();
    let base = base_owner(&coordinator);
    let fixture_owner = coordinator
        .register_provider(
            ProviderId(2),
            &[SurfaceSpec::new(
                101,
                SurfaceRole::AppRoot,
                SurfaceCapabilities::LAUNCHABLE,
                RefreshHint::Boundary,
            )],
        )
        .unwrap();

    navigate(
        &mut coordinator,
        NavIntent::OpenLauncher(surface(base, LAUNCHER)),
        &mut screens,
        &mut overlays,
    );
    navigate(
        &mut coordinator,
        NavIntent::Launch(surface(fixture_owner, 101)),
        &mut screens,
        &mut overlays,
    );

    // The Home fallback candidate this removal must enter now fails to
    // activate, and its best-effort cleanup destroy reports an audit
    // mismatch rather than a still-live object.
    screens.activate_ok = false;
    screens.destroy_audit_failed = true;

    let outcome = coordinator
        .dispatch_provider_removal(fixture_owner, &mut screens, &mut overlays)
        .unwrap();
    assert!(matches!(outcome, ProviderRemovalDispatchOutcome::Rejected));

    assert!(coordinator.lifecycle_audit_faulted());
    assert!(coordinator.navigation_faulted());
    assert!(coordinator.is_faulted());
    // The audit branch reports a mismatch, not a still-live instance -- it
    // must not also be reported as a retryable blocked cleanup.
    assert!(coordinator.cleanup_blocked_screen().is_none());
}

#[test]
fn stale_composition_source_is_rejected() {
    let (mut coordinator, mut screens, mut overlays) = coordinator();
    let owner = base_owner(&coordinator);
    let stale_source = coordinator.shell().active_instance();

    navigate(
        &mut coordinator,
        NavIntent::OpenLauncher(surface(owner, LAUNCHER)),
        &mut screens,
        &mut overlays,
    );
    // `stale_source` named Home, which is no longer active; a composition
    // request built from it must be rejected, not silently attributed to
    // whatever screen happens to be active now.
    let result = coordinator.dispatch_composition(
        OwnedCompositionIntent {
            source: stale_source,
            intent: CompositionIntent::Request {
                surface: surface(owner, PASSIVE_OVERLAY),
                input: OverlayInput::Passive,
                lifetime: OverlayLifetime::Sticky,
                rank: 1,
            },
        },
        &mut overlays,
    );
    assert_eq!(
        result,
        Err(DispatchRejected::Composition(
            CompositionError::InvalidSource(stale_source)
        ))
    );
    assert_eq!(coordinator.live_overlay_len(), 0);
}

#[test]
fn overlay_capacity_limit_is_reported_not_corrupted() {
    type TinyCoordinator = UiCoordinator<FakeScreen, FakeOverlay, 8, 20, 8, 1, 4, 8>;
    let mut coordinator =
        TinyCoordinator::new(BASE_PROVIDER, &base_specs(), SurfaceId(HOME)).unwrap();
    let mut screens = FakeScreenRuntime::healthy();
    let mut overlays = FakeOverlayRuntime::healthy();
    coordinator.bootstrap_screen(&mut screens).unwrap();
    let owner = coordinator.shell().active().surface.owner;
    let source = coordinator.shell().active_instance();

    let first = coordinator
        .dispatch_composition(
            OwnedCompositionIntent {
                source,
                intent: CompositionIntent::Request {
                    surface: surface(owner, PASSIVE_OVERLAY),
                    input: OverlayInput::Passive,
                    lifetime: OverlayLifetime::Sticky,
                    rank: 1,
                },
            },
            &mut overlays,
        )
        .unwrap();
    assert!(matches!(
        first,
        CompositionDispatchOutcome::Admitted(OverlayAdmission::Active(_))
    ));

    let second = coordinator.dispatch_composition(
        OwnedCompositionIntent {
            source,
            intent: CompositionIntent::Request {
                surface: surface(owner, INTERACTIVE_OVERLAY),
                input: OverlayInput::Interactive,
                lifetime: OverlayLifetime::Sticky,
                rank: 1,
            },
        },
        &mut overlays,
    );
    assert_eq!(
        second,
        Err(DispatchRejected::Composition(CompositionError::Reference(
            crate::composition::CompositionReferenceError::LiveOverlayCapacity
        )))
    );
    // Rejected before any target effect: the runtime never saw a second `enter`.
    assert_eq!(overlays.live, 1);
    assert_eq!(coordinator.live_overlay_len(), 1);
}

#[test]
fn event_queued_during_a_change_is_not_lost_and_not_misattributed() {
    let (mut coordinator, mut screens, mut overlays) = coordinator();
    let owner = base_owner(&coordinator);
    let home_instance = coordinator.shell().active_instance();

    // Simulate a callback firing on the currently-visible screen while
    // (from the product's perspective) some other, unrelated dispatch is
    // in flight: queue it through `ShellModel` directly, the same path a
    // real callback route drains through later.
    coordinator
        .shell
        .queue_intent(OwnedNavIntent {
            source: home_instance,
            intent: NavIntent::OpenLauncher(surface(owner, LAUNCHER)),
        })
        .unwrap();

    // A second, unrelated navigation runs to completion first...
    navigate(
        &mut coordinator,
        NavIntent::OpenLauncher(surface(owner, LAUNCHER)),
        &mut screens,
        &mut overlays,
    );
    assert_eq!(
        coordinator.shell().active().surface,
        surface(owner, LAUNCHER)
    );

    // ...and the queued intent, now stale (its source was Home, not the
    // active Launcher instance), is purged rather than silently applied
    // against the wrong screen: draining it must find nothing left.
    assert!(coordinator.shell.pop_intent().is_none());
}

#[test]
fn failed_overlay_enable_rolls_back_without_touching_the_shell() {
    let (mut coordinator, _screens, mut overlays) = coordinator();
    let owner = base_owner(&coordinator);
    let revision_before = coordinator.shell().live_overlay_len();
    overlays.enable_ok = false;

    let source = coordinator.shell().active_instance();
    let outcome = coordinator
        .dispatch_composition(
            OwnedCompositionIntent {
                source,
                intent: CompositionIntent::Request {
                    surface: surface(owner, PASSIVE_OVERLAY),
                    input: OverlayInput::Passive,
                    lifetime: OverlayLifetime::Sticky,
                    rank: 1,
                },
            },
            &mut overlays,
        )
        .unwrap();
    assert_eq!(
        outcome,
        CompositionDispatchOutcome::RolledBack(CompositionRollbackReason::Entry)
    );
    assert_eq!(coordinator.shell().live_overlay_len(), revision_before);
    assert_eq!(coordinator.live_overlay_len(), 0);
    // The candidate was entered, failed `enable`, and was torn down again.
    assert_eq!(overlays.live, 0);
    assert!(!coordinator.is_faulted());
}

/// `enable_live` is what every composition/navigation/provider-removal
/// rollback path calls to restore an overlay it disabled while staging a
/// change that then failed later. Its `bool` return is the same
/// "did the rollback actually work" bookkeeping `execute_transition`
/// keeps for a screen's `origin_restored` -- this exercises it directly
/// rather than needing to reproduce a full shell-commit failure just to
/// reach the call site.
#[test]
fn enable_live_reports_whether_every_token_was_actually_restored() {
    let (mut coordinator, _screens, mut overlays) = coordinator();
    let owner = base_owner(&coordinator);
    let source = coordinator.shell().active_instance();

    let outcome = coordinator
        .dispatch_composition(
            OwnedCompositionIntent {
                source,
                intent: CompositionIntent::Request {
                    surface: surface(owner, PASSIVE_OVERLAY),
                    input: OverlayInput::Passive,
                    lifetime: OverlayLifetime::Sticky,
                    rank: 1,
                },
            },
            &mut overlays,
        )
        .unwrap();
    let CompositionDispatchOutcome::Admitted(OverlayAdmission::Active(instance)) = outcome else {
        panic!("a healthy request must admit live immediately");
    };
    let token = instance.token;

    // A healthy disable-then-enable round-trip reports full restoration.
    assert!(coordinator.disable_live(&[token], &mut overlays));
    assert!(coordinator.enable_live(&[token], &mut overlays));

    // Disable again, then break `enable` before the rollback restores it
    // -- the same "disabled and never actually re-enabled" state a
    // failed shell commit would otherwise leave silently unreported.
    assert!(coordinator.disable_live(&[token], &mut overlays));
    overlays.enable_ok = false;
    assert!(!coordinator.enable_live(&[token], &mut overlays));

    // A token with no live overlay behind it at all is reported the
    // same way -- nothing to restore is still not "restored".
    overlays.enable_ok = true;
    let missing_token =
        SurfaceInstanceToken::issued(surface(owner, PASSIVE_OVERLAY), InstanceGeneration(9999));
    assert!(!coordinator.enable_live(&[missing_token], &mut overlays));
}

#[test]
fn retained_cleanup_fault_blocks_dispatch_until_retried() {
    let (mut coordinator, mut screens, mut overlays) = coordinator();
    let owner = base_owner(&coordinator);
    screens.destroy_ok = false;

    let outcome = navigate(
        &mut coordinator,
        NavIntent::OpenLauncher(surface(owner, LAUNCHER)),
        &mut screens,
        &mut overlays,
    );
    // The screen swap itself commits -- Launcher is live and Home's
    // `destroy` is what fails -- so this is a retained cleanup fault
    // after a successful commit, not a rollback.
    assert_eq!(outcome, NavigationDispatchOutcome::FaultedAfterCommit);
    assert_eq!(
        coordinator.shell().active().surface,
        surface(owner, LAUNCHER)
    );
    assert!(coordinator.is_faulted());
    assert_eq!(
        coordinator.dispatch_navigation(NavIntent::Home, &mut screens, &mut overlays),
        Err(DispatchRejected::Faulted)
    );

    screens.destroy_ok = true;
    coordinator.retry_blocked_cleanup(&mut screens, &mut overlays);
    assert!(!coordinator.is_faulted());
    assert_eq!(
        navigate(
            &mut coordinator,
            NavIntent::Home,
            &mut screens,
            &mut overlays
        ),
        NavigationDispatchOutcome::Committed { changed: true }
    );
}
