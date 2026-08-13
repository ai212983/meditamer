use core::mem::size_of;

use super::{
    callback_action_queue::CallbackActionQueue,
    composition::{CompositionReferenceError, CompositionReferences},
    intent_queue::IntentQueueError,
    model::{
        CompositionError, DefaultShellModel, ProviderRegistrationError, ProviderRemovalError,
        ProviderRuntimeAudit, ShellModel, ShellNavigationError, DEFAULT_SHELL_MODEL_BYTES,
        FUTURE_RETAINED_MODEL_REFERENCE_CEILING, LIVE_OVERLAY_CAPACITY, MODAL_QUEUE_CAPACITY,
        NAVIGATION_STACK_CAPACITY, PROVIDER_CAPACITY, RETAINED_MODEL_CAPACITY,
        SHELL_INTENT_QUEUE_CAPACITY, SURFACE_REGISTRY_CAPACITY,
    },
    navigator::{NavigationError, NavigationOutcome},
    registry::{RegistrationError, ResolveError},
    types::{
        CompositionIntent, InstanceGeneration, NavIntent, OverlayAdmission, OverlayBand,
        OverlayInput, OverlayLifetime, OwnedCompositionIntent, OwnedNavIntent, OwnedRefreshIntent,
        OwnedShellIntent, ProviderId, ProviderToken, RefreshHint, RefreshIntent,
        SurfaceCapabilities, SurfaceId, SurfaceInstanceToken, SurfaceRef, SurfaceRole, SurfaceSpec,
    },
};

const BASE_PROVIDER: ProviderId = ProviderId(10);
const AMBIENT_ID: u16 = 101;
const LAUNCHER_ID: u16 = 207;

type TestShell = ShellModel<8, 16, 8, 4, 4, 8>;

fn spec(id: u16, role: SurfaceRole) -> SurfaceSpec {
    let capabilities = match role {
        SurfaceRole::Ambient => SurfaceCapabilities::AMBIENT,
        SurfaceRole::AppRoot | SurfaceRole::SystemRoot => SurfaceCapabilities::LAUNCHABLE,
        SurfaceRole::Overlay => SurfaceCapabilities::OVERLAY,
        SurfaceRole::Launcher | SurfaceRole::AppChild => SurfaceCapabilities::NONE,
    };
    SurfaceSpec::new(id, role, capabilities, RefreshHint::Content)
}

fn base_specs() -> [SurfaceSpec; 2] {
    [
        spec(AMBIENT_ID, SurfaceRole::Ambient),
        spec(LAUNCHER_ID, SurfaceRole::Launcher),
    ]
}

fn shell() -> TestShell {
    TestShell::new(BASE_PROVIDER, &base_specs(), SurfaceId(AMBIENT_ID)).unwrap()
}

fn base_owner(shell: &TestShell) -> ProviderToken {
    shell.active().surface.owner
}

fn surface(owner: ProviderToken, id: u16) -> SurfaceRef {
    SurfaceRef::new(owner, id)
}

fn commit(shell: &mut TestShell, intent: NavIntent) -> NavigationOutcome {
    let prepared = shell.prepare_intent(intent).unwrap();
    shell.commit_navigation(prepared).unwrap()
}

#[test]
fn registration_is_atomic_and_surface_ids_are_globally_unique() {
    type SmallShell = ShellModel<3, 4, 4, 1, 1, 1>;
    let mut shell = SmallShell::new(BASE_PROVIDER, &base_specs(), SurfaceId(AMBIENT_ID)).unwrap();
    let definitions_before = shell.definition_len();
    let providers_before = shell.provider_len();

    let duplicate_batch = [
        spec(301, SurfaceRole::AppRoot),
        spec(301, SurfaceRole::AppChild),
    ];
    assert_eq!(
        shell.register_provider(ProviderId(20), &duplicate_batch),
        Err(ProviderRegistrationError::Registration(
            RegistrationError::DuplicateSurfaceIdInBatch(SurfaceId(301))
        ))
    );
    assert_eq!(shell.definition_len(), definitions_before);
    assert_eq!(shell.provider_len(), providers_before);

    let owner = shell
        .register_provider(ProviderId(20), &[spec(301, SurfaceRole::AppRoot)])
        .unwrap();
    assert_eq!(
        shell.register_provider(ProviderId(30), &[spec(301, SurfaceRole::AppRoot)]),
        Err(ProviderRegistrationError::Registration(
            RegistrationError::DuplicateSurfaceId {
                registered: surface(owner, 301),
                requested: SurfaceId(301),
            }
        ))
    );
    assert_eq!(shell.provider_len(), providers_before + 1);
}

#[test]
fn provider_and_surface_capacity_fail_without_partial_registration() {
    type ProviderLimited = ShellModel<2, 8, 4, 1, 1, 1>;
    let mut provider_limited =
        ProviderLimited::new(BASE_PROVIDER, &base_specs(), SurfaceId(AMBIENT_ID)).unwrap();
    provider_limited
        .register_provider(ProviderId(20), &[spec(301, SurfaceRole::AppRoot)])
        .unwrap();
    assert_eq!(
        provider_limited.register_provider(ProviderId(30), &[spec(401, SurfaceRole::AppRoot)]),
        Err(ProviderRegistrationError::Registration(
            RegistrationError::ProviderCapacity
        ))
    );

    type SurfaceLimited = ShellModel<3, 3, 4, 1, 1, 1>;
    let mut surface_limited =
        SurfaceLimited::new(BASE_PROVIDER, &base_specs(), SurfaceId(AMBIENT_ID)).unwrap();
    assert_eq!(
        surface_limited.register_provider(
            ProviderId(20),
            &[
                spec(301, SurfaceRole::AppRoot),
                spec(302, SurfaceRole::AppChild),
            ]
        ),
        Err(ProviderRegistrationError::Registration(
            RegistrationError::SurfaceCapacity
        ))
    );
    assert_eq!(surface_limited.definition_len(), 2);
    assert_eq!(surface_limited.provider_len(), 1);
}

#[test]
fn navigation_uses_roles_for_back_home_and_app_replacement() {
    let mut shell = shell();
    let base = base_owner(&shell);
    let first = shell
        .register_provider(
            ProviderId(55),
            &[
                spec(701, SurfaceRole::AppRoot),
                spec(809, SurfaceRole::AppChild),
            ],
        )
        .unwrap();
    let second = shell
        .register_provider(ProviderId(77), &[spec(911, SurfaceRole::SystemRoot)])
        .unwrap();

    assert_eq!(
        commit(
            &mut shell,
            NavIntent::OpenLauncher(surface(base, LAUNCHER_ID))
        ),
        NavigationOutcome::Changed
    );
    commit(&mut shell, NavIntent::Launch(surface(first, 701)));
    commit(&mut shell, NavIntent::Push(surface(first, 809)));
    assert_eq!(shell.navigation_len(), 4);

    commit(&mut shell, NavIntent::Back);
    assert_eq!(shell.active().surface, surface(first, 701));
    commit(&mut shell, NavIntent::Launch(surface(second, 911)));
    assert_eq!(shell.navigation_len(), 3);
    assert_eq!(shell.active().surface, surface(second, 911));

    commit(&mut shell, NavIntent::Home);
    assert_eq!(shell.navigation_len(), 1);
    assert_eq!(shell.active().surface, surface(base, AMBIENT_ID));
    assert_eq!(
        commit(&mut shell, NavIntent::Home),
        NavigationOutcome::Unchanged
    );
}

#[test]
fn dropped_and_stale_navigation_plans_preserve_the_previous_stack() {
    let mut shell = shell();
    let base = base_owner(&shell);
    let app = shell
        .register_provider(ProviderId(55), &[spec(701, SurfaceRole::AppRoot)])
        .unwrap();
    commit(
        &mut shell,
        NavIntent::OpenLauncher(surface(base, LAUNCHER_ID)),
    );
    let before = [shell.navigation_frame(0), shell.navigation_frame(1)];

    let failed_entry_plan = shell
        .prepare_intent(NavIntent::Launch(surface(app, 701)))
        .unwrap();
    assert_eq!(failed_entry_plan.destination().surface, surface(app, 701));
    drop(failed_entry_plan);
    assert_eq!(shell.navigation_len(), 2);
    assert_eq!(
        [shell.navigation_frame(0), shell.navigation_frame(1)],
        before
    );

    let stale = shell.prepare_intent(NavIntent::Back).unwrap();
    let source = shell.active_instance();
    shell
        .queue_intent(OwnedNavIntent {
            source,
            intent: NavIntent::Home,
        })
        .unwrap();
    assert_eq!(
        shell.commit_navigation(stale),
        Err(ShellNavigationError::StalePlan)
    );
    assert_eq!(shell.navigation_len(), 2);
}

#[test]
fn stack_composition_and_intent_capacities_are_explicit() {
    type TinyShell = ShellModel<4, 8, 3, 1, 1, 1>;
    let mut shell = TinyShell::new(BASE_PROVIDER, &base_specs(), SurfaceId(AMBIENT_ID)).unwrap();
    let base = shell.active().surface.owner;
    let app = shell
        .register_provider(
            ProviderId(55),
            &[
                spec(701, SurfaceRole::AppRoot),
                spec(702, SurfaceRole::AppChild),
                spec(703, SurfaceRole::Overlay),
                spec(704, SurfaceRole::Overlay),
            ],
        )
        .unwrap();

    let plan = shell
        .prepare_intent(NavIntent::OpenLauncher(surface(base, LAUNCHER_ID)))
        .unwrap();
    shell.commit_navigation(plan).unwrap();
    let plan = shell
        .prepare_intent(NavIntent::Launch(surface(app, 701)))
        .unwrap();
    shell.commit_navigation(plan).unwrap();
    assert!(matches!(
        shell.prepare_intent(NavIntent::Push(surface(app, 702))),
        Err(ShellNavigationError::Navigation(NavigationError::Capacity))
    ));
    assert_eq!(shell.navigation_len(), 3);

    assert!(matches!(
        shell.request_overlay(
            surface(app, 703),
            OverlayInput::Modal,
            OverlayLifetime::Transient,
            1,
        ),
        Ok(OverlayAdmission::Active(_))
    ));
    assert_eq!(
        shell.request_overlay(
            surface(app, 704),
            OverlayInput::Passive,
            OverlayLifetime::Transient,
            1,
        ),
        Err(super::model::CompositionError::Reference(
            CompositionReferenceError::LiveOverlayCapacity
        ))
    );
    assert_eq!(shell.live_overlay_len(), 1);
    assert!(matches!(
        shell.request_overlay(
            surface(app, 703),
            OverlayInput::Modal,
            OverlayLifetime::Transient,
            1,
        ),
        Ok(OverlayAdmission::Queued(_))
    ));
    assert_eq!(
        shell.request_overlay(
            surface(app, 704),
            OverlayInput::Modal,
            OverlayLifetime::Transient,
            1,
        ),
        Err(super::model::CompositionError::Reference(
            CompositionReferenceError::ModalQueueCapacity
        ))
    );
    assert_eq!(shell.live_overlay_len(), 1);
    assert_eq!(shell.queued_modal_len(), 1);
    let source = shell.active_instance();
    shell
        .queue_intent(OwnedNavIntent {
            source,
            intent: NavIntent::Home,
        })
        .unwrap();
    assert_eq!(
        shell.queue_intent(OwnedNavIntent {
            source,
            intent: NavIntent::Back,
        }),
        Err(IntentQueueError::Capacity)
    );
    assert_eq!(
        shell.pop_intent(),
        Some(OwnedNavIntent {
            source,
            intent: NavIntent::Home,
        })
    );
}

#[test]
fn generations_are_shell_issued_and_stale_references_are_rejected() {
    let mut shell = shell();
    let first = shell
        .register_provider(ProviderId(55), &[spec(701, SurfaceRole::AppRoot)])
        .unwrap();
    let removal = shell.prepare_provider_removal(first).unwrap();
    shell.commit_provider_removal(removal).unwrap();
    let second = shell
        .register_provider(ProviderId(55), &[spec(701, SurfaceRole::AppRoot)])
        .unwrap();
    assert_eq!(second.id, first.id);
    assert!(second.generation.0 > first.generation.0);
    assert_eq!(
        shell.queue_intent(OwnedNavIntent {
            source: SurfaceInstanceToken::issued(surface(first, 701), InstanceGeneration(1),),
            intent: NavIntent::Home,
        }),
        Err(IntentQueueError::InvalidOwner)
    );
    match shell.prepare_intent(NavIntent::Launch(surface(first, 701))) {
        Err(ShellNavigationError::Resolve(ResolveError::StaleProviderGeneration {
            active,
            requested,
        })) => {
            assert_eq!(active, Some(second));
            assert_eq!(requested, first);
        }
        _ => panic!("old provider generation was not rejected explicitly"),
    }
}

#[test]
fn recreated_surface_rejects_intents_from_the_deleted_instance() {
    let mut shell = shell();
    let base = base_owner(&shell);
    let first_home = shell.active_instance();

    commit(
        &mut shell,
        NavIntent::OpenLauncher(surface(base, LAUNCHER_ID)),
    );
    commit(&mut shell, NavIntent::Home);
    let second_home = shell.active_instance();

    assert_eq!(second_home.surface, first_home.surface);
    assert!(second_home.generation.0 > first_home.generation.0);
    assert_eq!(
        shell.queue_intent(OwnedNavIntent {
            source: first_home,
            intent: NavIntent::Home,
        }),
        Err(IntentQueueError::StaleInstance)
    );
}

#[test]
fn recovery_home_recreates_even_when_home_is_already_active() {
    let mut shell = shell();
    let first_home = shell.active_instance();
    let recovery = shell.prepare_recovery_home().unwrap();

    assert_eq!(recovery.destination().surface, first_home.surface);
    assert!(recovery.requires_reentry(first_home));
    let recovered_home = recovery.destination_instance();
    assert_ne!(recovered_home, first_home);
    assert_eq!(
        shell.commit_navigation(recovery).unwrap(),
        NavigationOutcome::Unchanged
    );
    assert_eq!(shell.active_instance(), recovered_home);
    assert_eq!(
        shell.queue_intent(OwnedNavIntent {
            source: first_home,
            intent: NavIntent::Home,
        }),
        Err(IntentQueueError::StaleInstance)
    );
}

#[test]
fn static_vertical_slice_uses_ambient_launcher_system_root_topology() {
    let mut shell = shell();
    let base = base_owner(&shell);
    let diagnostics = shell
        .register_provider(ProviderId(55), &[spec(943, SurfaceRole::SystemRoot)])
        .unwrap();

    commit(
        &mut shell,
        NavIntent::OpenLauncher(surface(base, LAUNCHER_ID)),
    );
    assert_eq!(shell.active().role, SurfaceRole::Launcher);
    commit(&mut shell, NavIntent::Launch(surface(diagnostics, 943)));
    assert_eq!(shell.active().role, SurfaceRole::SystemRoot);
    commit(&mut shell, NavIntent::Back);
    assert_eq!(shell.active().role, SurfaceRole::Launcher);
    commit(&mut shell, NavIntent::Home);
    assert_eq!(shell.active().role, SurfaceRole::Ambient);
}

#[test]
fn provider_removal_is_transactional_and_purges_all_owned_references() {
    let mut shell = shell();
    let base = base_owner(&shell);
    let app = shell
        .register_provider(
            ProviderId(55),
            &[
                spec(701, SurfaceRole::AppRoot),
                spec(702, SurfaceRole::AppChild),
                spec(703, SurfaceRole::Overlay),
            ],
        )
        .unwrap();
    commit(
        &mut shell,
        NavIntent::OpenLauncher(surface(base, LAUNCHER_ID)),
    );
    commit(&mut shell, NavIntent::Launch(surface(app, 701)));
    commit(&mut shell, NavIntent::Push(surface(app, 702)));
    assert!(matches!(
        shell.request_overlay(
            surface(app, 703),
            OverlayInput::Modal,
            OverlayLifetime::Transient,
            1,
        ),
        Ok(OverlayAdmission::Active(_))
    ));
    assert!(matches!(
        shell.request_overlay(
            surface(app, 703),
            OverlayInput::Modal,
            OverlayLifetime::Transient,
            1,
        ),
        Ok(OverlayAdmission::Queued(_))
    ));
    let source = shell.active_instance();
    shell
        .queue_intent(OwnedNavIntent {
            source,
            intent: NavIntent::Back,
        })
        .unwrap();

    let failed_fallback = shell.prepare_provider_removal(app).unwrap();
    let fallback = failed_fallback.fallback_transition().unwrap();
    assert_eq!(fallback.origin.surface, surface(app, 702));
    assert_eq!(fallback.destination.surface, surface(base, AMBIENT_ID));
    assert_eq!(
        fallback.destination_instance.surface,
        fallback.destination.surface
    );
    drop(failed_fallback);
    assert_eq!(shell.navigation_len(), 4);
    assert_eq!(shell.live_overlay_len(), 1);
    assert_eq!(shell.queued_modal_len(), 1);
    assert_eq!(shell.queued_intent_len(), 1);

    let plan = shell.prepare_provider_removal(app).unwrap();
    let fallback_instance = plan
        .fallback_transition()
        .expect("active provider removal must prepare fallback")
        .destination_instance;
    let definitions_before_detach = shell.definition_len();
    let pending = shell.commit_provider_detach(plan).unwrap();
    assert_eq!(pending.owner(), app);
    assert_eq!(shell.definition_len(), definitions_before_detach);
    assert!(matches!(
        shell.prepare_intent(NavIntent::Home),
        Err(ShellNavigationError::ProviderRemovalInProgress)
    ));
    assert_eq!(
        shell.queue_intent(OwnedNavIntent {
            source: shell.active_instance(),
            intent: NavIntent::Home,
        }),
        Err(IntentQueueError::ProviderRemovalInProgress)
    );
    assert_eq!(
        shell.register_provider(ProviderId(77), &[spec(801, SurfaceRole::AppRoot)]),
        Err(ProviderRegistrationError::ProviderRemovalInProgress)
    );
    assert!(matches!(
        shell.prepare_provider_removal(app),
        Err(ProviderRemovalError::RemovalInProgress)
    ));
    let purge = shell
        .finalize_provider_removal(&pending, ProviderRuntimeAudit::verified(pending.owner()))
        .unwrap();
    assert_eq!(purge.definitions, 3);
    assert_eq!(purge.navigation.removed_frames, 3);
    assert!(purge.navigation.active_changed);
    assert_eq!(purge.composition.live_overlays, 1);
    assert_eq!(purge.composition.queued_modals, 1);
    assert_eq!(purge.queued_intents, 1);
    assert_eq!(shell.navigation_len(), 1);
    assert_eq!(shell.live_overlay_len(), 0);
    assert_eq!(shell.queued_modal_len(), 0);
    assert_eq!(shell.queued_intent_len(), 0);
    assert_eq!(shell.active_instance(), fallback_instance);
}

#[test]
fn provider_detach_purges_target_intents_and_provider_requested_base_overlays() {
    let base_specs = [
        spec(AMBIENT_ID, SurfaceRole::Ambient),
        spec(LAUNCHER_ID, SurfaceRole::Launcher),
        spec(301, SurfaceRole::Overlay),
        spec(302, SurfaceRole::Overlay),
    ];
    let mut shell = TestShell::new(BASE_PROVIDER, &base_specs, SurfaceId(AMBIENT_ID)).unwrap();
    let base = base_owner(&shell);
    let provider = shell
        .register_provider(ProviderId(55), &[spec(701, SurfaceRole::AppRoot)])
        .unwrap();
    commit(
        &mut shell,
        NavIntent::OpenLauncher(surface(base, LAUNCHER_ID)),
    );
    let OverlayAdmission::Active(unrelated_transient) = shell
        .request_overlay(
            surface(base, 301),
            OverlayInput::Passive,
            OverlayLifetime::Transient,
            1,
        )
        .unwrap()
    else {
        panic!("unrelated base transient must be live")
    };
    let launcher = shell.active_instance();
    shell
        .queue_intent(OwnedNavIntent {
            source: launcher,
            intent: NavIntent::Launch(surface(provider, 701)),
        })
        .unwrap();
    let target_plan = shell.prepare_provider_removal(provider).unwrap();
    assert!(target_plan.composition_delta().leave_live().is_empty());
    let pending = shell.commit_provider_detach(target_plan).unwrap();
    assert!(matches!(
        shell.prepare_transient_overlay_removal(),
        Err(CompositionError::ProviderRemovalInProgress)
    ));
    assert_eq!(
        shell.finalize_provider_removal(&pending, ProviderRuntimeAudit::verified(base)),
        Err(ProviderRemovalError::RuntimeAuditMismatch)
    );
    let target_purge = shell
        .finalize_provider_removal(&pending, ProviderRuntimeAudit::verified(pending.owner()))
        .unwrap();
    assert_eq!(target_purge.queued_intents, 1);
    assert_eq!(shell.live_overlay(0), Some(unrelated_transient));

    let provider = shell
        .register_provider(ProviderId(55), &[spec(701, SurfaceRole::AppRoot)])
        .unwrap();
    commit(&mut shell, NavIntent::Launch(surface(provider, 701)));
    let provider_screen = shell.active_instance();
    let first = shell
        .prepare_composition_intent(OwnedCompositionIntent {
            source: provider_screen,
            intent: CompositionIntent::Request {
                surface: surface(base, 301),
                input: OverlayInput::Modal,
                lifetime: OverlayLifetime::Sticky,
                rank: 1,
            },
        })
        .unwrap();
    let OverlayAdmission::Active(first) = shell.commit_overlay_request(first).unwrap() else {
        panic!("first provider-requested base modal must be active")
    };
    let second = shell
        .prepare_composition_intent(OwnedCompositionIntent {
            source: provider_screen,
            intent: CompositionIntent::Request {
                surface: surface(base, 302),
                input: OverlayInput::Modal,
                lifetime: OverlayLifetime::Sticky,
                rank: 1,
            },
        })
        .unwrap();
    let OverlayAdmission::Queued(second) = shell.commit_overlay_request(second).unwrap() else {
        panic!("second provider-requested base modal must queue")
    };
    assert_eq!(first.request_owner, provider);
    assert_eq!(second.request_owner, provider);

    let plan = shell.prepare_provider_removal(provider).unwrap();
    assert_eq!(plan.owner(), provider);
    assert_eq!(plan.composition_delta().leave_live(), &[first]);
    assert_eq!(plan.composition_delta().remove_queued(), &[second]);
    let definitions_before_detach = shell.definition_len();
    let pending = shell.commit_provider_detach(plan).unwrap();
    assert_eq!(shell.definition_len(), definitions_before_detach);
    assert_eq!(shell.live_overlay_len(), 0);
    assert_eq!(shell.queued_modal_len(), 0);
    shell
        .finalize_provider_removal(&pending, ProviderRuntimeAudit::verified(pending.owner()))
        .unwrap();
    assert_eq!(shell.active().surface, surface(base, AMBIENT_ID));
}

#[test]
fn provider_fallback_drops_base_transients_and_preserves_base_sticky_overlays() {
    let base_specs = [
        spec(AMBIENT_ID, SurfaceRole::Ambient),
        spec(LAUNCHER_ID, SurfaceRole::Launcher),
        spec(301, SurfaceRole::Overlay),
        spec(302, SurfaceRole::Overlay),
    ];
    let mut shell = TestShell::new(BASE_PROVIDER, &base_specs, SurfaceId(AMBIENT_ID)).unwrap();
    let base = base_owner(&shell);
    let provider = shell
        .register_provider(
            ProviderId(55),
            &[
                spec(701, SurfaceRole::AppRoot),
                spec(703, SurfaceRole::Overlay),
            ],
        )
        .unwrap();
    commit(
        &mut shell,
        NavIntent::OpenLauncher(surface(base, LAUNCHER_ID)),
    );
    commit(&mut shell, NavIntent::Launch(surface(provider, 701)));
    let OverlayAdmission::Active(transient) = shell
        .request_overlay(
            surface(base, 301),
            OverlayInput::Passive,
            OverlayLifetime::Transient,
            1,
        )
        .unwrap()
    else {
        panic!("base transient must be live")
    };
    let OverlayAdmission::Active(sticky) = shell
        .request_overlay(
            surface(base, 302),
            OverlayInput::Passive,
            OverlayLifetime::Sticky,
            2,
        )
        .unwrap()
    else {
        panic!("base sticky overlay must be live")
    };
    let OverlayAdmission::Active(provider_overlay) = shell
        .request_overlay(
            surface(provider, 703),
            OverlayInput::Passive,
            OverlayLifetime::Sticky,
            1,
        )
        .unwrap()
    else {
        panic!("provider sticky overlay must be live")
    };

    let plan = shell.prepare_provider_removal(provider).unwrap();
    let leaving = plan.composition_delta().leave_live();
    assert_eq!(leaving.len(), 2);
    assert!(leaving.contains(&transient));
    assert!(leaving.contains(&provider_overlay));
    assert!(!leaving.contains(&sticky));
    let pending = shell.commit_provider_detach(plan).unwrap();
    assert_eq!(shell.live_overlay_len(), 1);
    assert_eq!(shell.live_overlay(0), Some(sticky));
    shell
        .finalize_provider_removal(&pending, ProviderRuntimeAudit::verified(pending.owner()))
        .unwrap();
    assert_eq!(shell.active().surface, surface(base, AMBIENT_ID));
}

#[test]
fn overlay_order_keeps_base_above_provider_and_passive_entries_coexist() {
    let base_specs = [
        SurfaceSpec::new(
            AMBIENT_ID,
            SurfaceRole::Ambient,
            SurfaceCapabilities::AMBIENT,
            RefreshHint::Micro,
        ),
        spec(LAUNCHER_ID, SurfaceRole::Launcher),
        spec(301, SurfaceRole::Overlay),
    ];
    let mut shell = TestShell::new(BASE_PROVIDER, &base_specs, SurfaceId(AMBIENT_ID)).unwrap();
    let base = base_owner(&shell);
    let provider = shell
        .register_provider(ProviderId(55), &[spec(701, SurfaceRole::Overlay)])
        .unwrap();

    shell
        .request_overlay(
            surface(provider, 701),
            OverlayInput::Passive,
            OverlayLifetime::Sticky,
            u8::MAX,
        )
        .unwrap();
    shell
        .request_overlay(
            surface(base, 301),
            OverlayInput::Passive,
            OverlayLifetime::Transient,
            0,
        )
        .unwrap();

    assert_eq!(shell.live_overlay_len(), 2);
    assert_eq!(shell.live_overlay(0).unwrap().band, OverlayBand::Provider);
    assert_eq!(shell.live_overlay(1).unwrap().band, OverlayBand::BaseSystem);
    assert_eq!(shell.queued_modal_len(), 0);
    assert_eq!(shell.merged_refresh_hint(), RefreshHint::Content);
}

#[test]
fn modal_queue_is_fifo_and_repeated_surface_requests_get_unique_instances() {
    let base_specs = [
        SurfaceSpec::new(
            AMBIENT_ID,
            SurfaceRole::Ambient,
            SurfaceCapabilities::AMBIENT,
            RefreshHint::Micro,
        ),
        spec(LAUNCHER_ID, SurfaceRole::Launcher),
        SurfaceSpec::new(
            301,
            SurfaceRole::Overlay,
            SurfaceCapabilities::OVERLAY,
            RefreshHint::Micro,
        ),
        SurfaceSpec::new(
            302,
            SurfaceRole::Overlay,
            SurfaceCapabilities::OVERLAY,
            RefreshHint::Boundary,
        ),
    ];
    let mut shell = TestShell::new(BASE_PROVIDER, &base_specs, SurfaceId(AMBIENT_ID)).unwrap();
    let base = base_owner(&shell);
    let active = shell
        .request_overlay(
            surface(base, 301),
            OverlayInput::Modal,
            OverlayLifetime::Transient,
            1,
        )
        .unwrap();
    let queued_first = shell
        .request_overlay(
            surface(base, 302),
            OverlayInput::Modal,
            OverlayLifetime::Transient,
            1,
        )
        .unwrap();
    let queued_second = shell
        .request_overlay(
            surface(base, 301),
            OverlayInput::Modal,
            OverlayLifetime::Transient,
            1,
        )
        .unwrap();
    let OverlayAdmission::Active(active) = active else {
        panic!("first modal must become active")
    };
    let OverlayAdmission::Queued(queued_first) = queued_first else {
        panic!("second modal must queue")
    };
    let OverlayAdmission::Queued(queued_second) = queued_second else {
        panic!("third modal must queue")
    };
    assert_ne!(active.token, queued_second.token);
    assert_eq!(shell.active_modal(), Some(active));
    assert_eq!(shell.queued_modal(0), Some(queued_first));
    assert_eq!(shell.queued_modal(1), Some(queued_second));
    assert_eq!(shell.merged_refresh_hint(), RefreshHint::Micro);

    let first = shell.remove_overlay(active.token).unwrap();
    assert_eq!(first.removed, active);
    assert_eq!(first.promoted, Some(queued_first));
    assert_eq!(shell.active_modal(), Some(queued_first));
    assert_eq!(shell.merged_refresh_hint(), RefreshHint::Boundary);
    let second = shell.dismiss_active_modal().unwrap();
    assert_eq!(second.removed, queued_first);
    assert_eq!(second.promoted, Some(queued_second));
}

#[test]
fn base_modal_preempts_provider_and_base_fifo_stays_ahead_of_provider_fifo() {
    let base_specs = [
        spec(AMBIENT_ID, SurfaceRole::Ambient),
        spec(LAUNCHER_ID, SurfaceRole::Launcher),
        spec(301, SurfaceRole::Overlay),
        spec(302, SurfaceRole::Overlay),
    ];
    let mut shell = TestShell::new(BASE_PROVIDER, &base_specs, SurfaceId(AMBIENT_ID)).unwrap();
    let base = base_owner(&shell);
    let provider = shell
        .register_provider(ProviderId(55), &[spec(701, SurfaceRole::Overlay)])
        .unwrap();
    let OverlayAdmission::Active(provider_active) = shell
        .request_overlay(
            surface(provider, 701),
            OverlayInput::Modal,
            OverlayLifetime::Sticky,
            1,
        )
        .unwrap()
    else {
        panic!("the first provider modal must become active")
    };

    let prepared = shell
        .prepare_overlay_request(
            surface(base, 301),
            OverlayInput::Modal,
            OverlayLifetime::Transient,
            1,
        )
        .unwrap();
    let super::composition::CompositionPlanResult::Admission(OverlayAdmission::Active(base_active)) =
        prepared.result()
    else {
        panic!("a base modal must preempt an active provider modal")
    };
    assert_eq!(prepared.delta().leave_live(), &[provider_active]);
    assert_eq!(prepared.delta().enter_live(), &[base_active]);
    assert!(prepared.delta().remove_queued().is_empty());
    assert_eq!(
        shell.commit_overlay_request(prepared).unwrap(),
        OverlayAdmission::Active(base_active)
    );
    assert_eq!(shell.active_modal(), Some(base_active));
    assert_eq!(shell.queued_modal_len(), 0);

    let OverlayAdmission::Queued(provider_queued) = shell
        .request_overlay(
            surface(provider, 701),
            OverlayInput::Modal,
            OverlayLifetime::Sticky,
            1,
        )
        .unwrap()
    else {
        panic!("provider modal must queue behind the active base modal")
    };
    let OverlayAdmission::Queued(base_queued) = shell
        .request_overlay(
            surface(base, 302),
            OverlayInput::Modal,
            OverlayLifetime::Sticky,
            1,
        )
        .unwrap()
    else {
        panic!("a second base modal must queue")
    };
    assert_eq!(shell.queued_modal(0), Some(base_queued));
    assert_eq!(shell.queued_modal(1), Some(provider_queued));

    let first = shell.remove_overlay(base_active.token).unwrap();
    assert_eq!(first.promoted, Some(base_queued));
    let second = shell.remove_overlay(base_queued.token).unwrap();
    assert_eq!(second.promoted, Some(provider_queued));
}

#[test]
fn provider_queue_capacity_cannot_starve_a_base_confirmation() {
    let base_specs = [
        spec(AMBIENT_ID, SurfaceRole::Ambient),
        spec(LAUNCHER_ID, SurfaceRole::Launcher),
        spec(301, SurfaceRole::Overlay),
        spec(302, SurfaceRole::Overlay),
    ];
    let mut shell = TestShell::new(BASE_PROVIDER, &base_specs, SurfaceId(AMBIENT_ID)).unwrap();
    let base = base_owner(&shell);
    let provider = shell
        .register_provider(ProviderId(55), &[spec(701, SurfaceRole::Overlay)])
        .unwrap();
    assert!(matches!(
        shell.request_overlay(
            surface(base, 301),
            OverlayInput::Modal,
            OverlayLifetime::Sticky,
            1,
        ),
        Ok(OverlayAdmission::Active(_))
    ));
    let mut provider_queue = [None; 4];
    for queued in &mut provider_queue {
        let OverlayAdmission::Queued(instance) = shell
            .request_overlay(
                surface(provider, 701),
                OverlayInput::Modal,
                OverlayLifetime::Sticky,
                1,
            )
            .unwrap()
        else {
            panic!("provider modal must queue behind the base modal")
        };
        *queued = Some(instance);
    }

    let prepared = shell
        .prepare_overlay_request(
            surface(base, 302),
            OverlayInput::Modal,
            OverlayLifetime::Sticky,
            1,
        )
        .unwrap();
    let super::composition::CompositionPlanResult::Admission(OverlayAdmission::Queued(protected)) =
        prepared.result()
    else {
        panic!("the protected base modal must reserve queue capacity")
    };
    assert_eq!(
        prepared.delta().remove_queued(),
        &[provider_queue[3].unwrap()]
    );
    assert!(prepared.delta().enter_live().is_empty());
    assert_eq!(
        shell.commit_overlay_request(prepared).unwrap(),
        OverlayAdmission::Queued(protected)
    );
    assert_eq!(shell.queued_modal(0), Some(protected));
    for (index, provider) in provider_queue[..3].iter().enumerate() {
        assert_eq!(shell.queued_modal(index + 1), *provider);
    }
}

#[test]
fn navigation_cleanup_drops_transient_overlays_and_preserves_sticky_entries() {
    let base_specs = [
        spec(AMBIENT_ID, SurfaceRole::Ambient),
        spec(LAUNCHER_ID, SurfaceRole::Launcher),
        spec(301, SurfaceRole::Overlay),
        spec(302, SurfaceRole::Overlay),
        spec(303, SurfaceRole::Overlay),
        spec(304, SurfaceRole::Overlay),
    ];
    let mut shell = TestShell::new(BASE_PROVIDER, &base_specs, SurfaceId(AMBIENT_ID)).unwrap();
    let base = base_owner(&shell);
    shell
        .request_overlay(
            surface(base, 301),
            OverlayInput::Passive,
            OverlayLifetime::Transient,
            1,
        )
        .unwrap();
    let sticky = shell
        .request_overlay(
            surface(base, 302),
            OverlayInput::Interactive,
            OverlayLifetime::Sticky,
            1,
        )
        .unwrap();
    let transient_modal = shell
        .request_overlay(
            surface(base, 303),
            OverlayInput::Modal,
            OverlayLifetime::Transient,
            1,
        )
        .unwrap();
    let sticky_modal = shell
        .request_overlay(
            surface(base, 304),
            OverlayInput::Modal,
            OverlayLifetime::Sticky,
            1,
        )
        .unwrap();

    let OverlayAdmission::Active(sticky) = sticky else {
        panic!("interactive sticky overlay must be live")
    };
    let OverlayAdmission::Active(transient_modal) = transient_modal else {
        panic!("first modal must be active")
    };
    let OverlayAdmission::Queued(sticky_modal) = sticky_modal else {
        panic!("second modal must queue")
    };

    let dropped = shell
        .prepare_intent(NavIntent::OpenLauncher(surface(base, LAUNCHER_ID)))
        .unwrap();
    assert_eq!(dropped.composition_delta().leave_live().len(), 2);
    assert_eq!(dropped.composition_delta().enter_live(), &[sticky_modal]);
    assert!(dropped.composition_delta().remove_queued().is_empty());
    drop(dropped);
    assert_eq!(shell.active_modal(), Some(transient_modal));
    assert_eq!(shell.live_overlay_len(), 3);

    let navigation = shell
        .prepare_intent(NavIntent::OpenLauncher(surface(base, LAUNCHER_ID)))
        .unwrap();
    assert_eq!(
        shell.commit_navigation(navigation).unwrap(),
        NavigationOutcome::Changed
    );
    assert_ne!(shell.active_modal(), Some(transient_modal));
    assert_eq!(shell.active_modal(), Some(sticky_modal));
    assert_eq!(shell.live_overlay_len(), 2);
    assert_eq!(shell.live_overlay(0), Some(sticky));
}

#[test]
fn provider_overlay_purge_is_generation_exact_and_preserves_base_entries() {
    let base_specs = [
        spec(AMBIENT_ID, SurfaceRole::Ambient),
        spec(LAUNCHER_ID, SurfaceRole::Launcher),
        spec(301, SurfaceRole::Overlay),
        spec(302, SurfaceRole::Overlay),
    ];
    let mut shell = TestShell::new(BASE_PROVIDER, &base_specs, SurfaceId(AMBIENT_ID)).unwrap();
    let base = base_owner(&shell);
    shell
        .request_overlay(
            surface(base, 301),
            OverlayInput::Passive,
            OverlayLifetime::Sticky,
            1,
        )
        .unwrap();
    let first = shell
        .register_provider(ProviderId(55), &[spec(701, SurfaceRole::Overlay)])
        .unwrap();
    shell
        .request_overlay(
            surface(first, 701),
            OverlayInput::Modal,
            OverlayLifetime::Transient,
            1,
        )
        .unwrap();
    let base_modal = shell
        .request_overlay(
            surface(base, 302),
            OverlayInput::Modal,
            OverlayLifetime::Sticky,
            1,
        )
        .unwrap();
    assert!(matches!(base_modal, OverlayAdmission::Active(_)));
    let plan = shell.prepare_provider_removal(first).unwrap();
    let purge = shell.commit_provider_removal(plan).unwrap();
    assert_eq!(purge.composition.live_overlays, 0);
    assert_eq!(purge.composition.queued_modals, 0);
    assert_eq!(shell.live_overlay_len(), 2);
    assert_eq!(shell.live_overlay(0).unwrap().token.surface.owner, base);
    assert_eq!(shell.active_modal().unwrap().token.surface.owner, base);

    let replacement = shell
        .register_provider(ProviderId(55), &[spec(701, SurfaceRole::Overlay)])
        .unwrap();
    assert_ne!(replacement, first);
    shell
        .request_overlay(
            surface(replacement, 701),
            OverlayInput::Passive,
            OverlayLifetime::Sticky,
            1,
        )
        .unwrap();
    assert_eq!(shell.live_overlay_len(), 3);
}

#[test]
fn overlay_requests_reject_non_overlay_and_stale_provider_references() {
    let mut shell = shell();
    let base = base_owner(&shell);
    assert_eq!(
        shell.request_overlay(
            surface(base, AMBIENT_ID),
            OverlayInput::Passive,
            OverlayLifetime::Transient,
            1,
        ),
        Err(super::model::CompositionError::NotOverlay(surface(
            base, AMBIENT_ID
        )))
    );

    let provider = shell
        .register_provider(ProviderId(55), &[spec(701, SurfaceRole::Overlay)])
        .unwrap();
    let plan = shell.prepare_provider_removal(provider).unwrap();
    shell.commit_provider_removal(plan).unwrap();
    assert!(matches!(
        shell.request_overlay(
            surface(provider, 701),
            OverlayInput::Passive,
            OverlayLifetime::Transient,
            1,
        ),
        Err(super::model::CompositionError::Resolve(
            ResolveError::StaleProviderGeneration { requested, .. }
        )) if requested == provider
    ));
}

#[test]
fn overlay_admission_and_removal_plans_are_transactional_and_stale_safe() {
    let base_specs = [
        spec(AMBIENT_ID, SurfaceRole::Ambient),
        spec(LAUNCHER_ID, SurfaceRole::Launcher),
        spec(301, SurfaceRole::Overlay),
        spec(302, SurfaceRole::Overlay),
    ];
    let mut shell = TestShell::new(BASE_PROVIDER, &base_specs, SurfaceId(AMBIENT_ID)).unwrap();
    let base = base_owner(&shell);

    let dropped = shell
        .prepare_overlay_request(
            surface(base, 301),
            OverlayInput::Passive,
            OverlayLifetime::Transient,
            1,
        )
        .unwrap();
    assert!(dropped.requires_entry());
    assert_eq!(shell.live_overlay_len(), 0);
    drop(dropped);
    assert_eq!(shell.live_overlay_len(), 0);

    let stale_admission = shell
        .prepare_overlay_request(
            surface(base, 301),
            OverlayInput::Passive,
            OverlayLifetime::Transient,
            1,
        )
        .unwrap();
    let active = shell
        .request_overlay(
            surface(base, 302),
            OverlayInput::Passive,
            OverlayLifetime::Sticky,
            1,
        )
        .unwrap();
    assert_eq!(
        shell.commit_overlay_request(stale_admission),
        Err(super::model::CompositionError::StalePlan)
    );
    assert_eq!(shell.live_overlay_len(), 1);

    let OverlayAdmission::Active(active) = active else {
        panic!("passive overlay must be active")
    };
    let dropped_removal = shell.prepare_overlay_removal(active.token).unwrap();
    assert!(matches!(
        dropped_removal.result(),
        super::composition::CompositionPlanResult::Removal(dismissal)
            if dismissal.removed_was_live
    ));
    drop(dropped_removal);
    assert_eq!(shell.live_overlay(0), Some(active));

    let stale_removal = shell.prepare_overlay_removal(active.token).unwrap();
    shell
        .request_overlay(
            surface(base, 301),
            OverlayInput::Passive,
            OverlayLifetime::Transient,
            1,
        )
        .unwrap();
    assert_eq!(
        shell.commit_overlay_removal(stale_removal),
        Err(super::model::CompositionError::StalePlan)
    );
    assert!(shell
        .live_overlay(0)
        .is_some_and(|instance| instance.token == active.token));
}

#[test]
fn composition_intents_require_the_exact_active_screen_or_modal_instance() {
    let base_specs = [
        spec(AMBIENT_ID, SurfaceRole::Ambient),
        spec(LAUNCHER_ID, SurfaceRole::Launcher),
        spec(301, SurfaceRole::Overlay),
    ];
    let mut shell = TestShell::new(BASE_PROVIDER, &base_specs, SurfaceId(AMBIENT_ID)).unwrap();
    let screen = shell.active_instance();
    let request = CompositionIntent::Request {
        surface: surface(base_owner(&shell), 301),
        input: OverlayInput::Modal,
        lifetime: OverlayLifetime::Transient,
        rank: 1,
    };
    let prepared = shell
        .prepare_composition_intent(OwnedCompositionIntent {
            source: screen,
            intent: request,
        })
        .unwrap();
    let modal = match shell.commit_overlay_request(prepared).unwrap() {
        OverlayAdmission::Active(instance) => instance,
        OverlayAdmission::Queued(_) => panic!("the first modal must become active"),
    };

    assert!(matches!(
        shell.prepare_composition_intent(OwnedCompositionIntent {
            source: modal.token,
            intent: request,
        }),
        Err(CompositionError::InvalidSource(source)) if source == modal.token
    ));
    assert!(matches!(
        shell.prepare_composition_intent(OwnedCompositionIntent {
            source: screen,
            intent: CompositionIntent::DismissActiveModal,
        }),
        Err(CompositionError::InvalidSource(source)) if source == screen
    ));
    let dismissal = shell
        .prepare_composition_intent(OwnedCompositionIntent {
            source: modal.token,
            intent: CompositionIntent::DismissActiveModal,
        })
        .unwrap();
    assert!(matches!(
        shell.commit_overlay_removal(dismissal).unwrap(),
        super::composition::CompositionPlanResult::Removal(removed)
            if removed.removed == modal && removed.promoted.is_none()
    ));
}

#[test]
fn callback_actions_preserve_order_capacity_and_exact_instance_purge() {
    let base_specs = [
        spec(AMBIENT_ID, SurfaceRole::Ambient),
        spec(LAUNCHER_ID, SurfaceRole::Launcher),
        spec(301, SurfaceRole::Overlay),
    ];
    let mut shell = TestShell::new(BASE_PROVIDER, &base_specs, SurfaceId(AMBIENT_ID)).unwrap();
    let screen = shell.active_instance();
    let base = base_owner(&shell);
    let modal = match shell
        .request_overlay(
            surface(base, 301),
            OverlayInput::Modal,
            OverlayLifetime::Transient,
            1,
        )
        .unwrap()
    {
        OverlayAdmission::Active(instance) => instance,
        OverlayAdmission::Queued(_) => panic!("the first modal must become active"),
    };
    let dismiss = OwnedShellIntent::Compose(OwnedCompositionIntent {
        source: modal.token,
        intent: CompositionIntent::DismissActiveModal,
    });
    let navigate = OwnedShellIntent::Navigate(OwnedNavIntent {
        source: screen,
        intent: NavIntent::Home,
    });
    let refresh = OwnedShellIntent::Refresh(OwnedRefreshIntent {
        source: screen,
        intent: RefreshIntent::FullRepaint,
    });

    let mut queue = CallbackActionQueue::<2>::new();
    queue.push(dismiss).unwrap();
    queue.push(navigate).unwrap();
    assert_eq!(queue.push(dismiss), Err(dismiss));
    assert_eq!(queue.pop(), Some(dismiss));
    assert_eq!(queue.pop(), Some(navigate));
    assert_eq!(queue.pop(), None);

    queue.push(refresh).unwrap();
    assert_eq!(queue.purge_instance(screen), 1);
    assert_eq!(queue.pop(), None);

    queue.push(navigate).unwrap();
    queue.push(dismiss).unwrap();
    assert_eq!(queue.purge_instance(modal.token), 1);
    assert_eq!(queue.pop(), Some(navigate));
    assert_eq!(queue.pop(), None);

    let provider = shell
        .register_provider(ProviderId(55), &[spec(701, SurfaceRole::AppRoot)])
        .unwrap();
    let target_provider = OwnedShellIntent::Navigate(OwnedNavIntent {
        source: screen,
        intent: NavIntent::Launch(surface(provider, 701)),
    });
    queue.push(target_provider).unwrap();
    assert_eq!(queue.provider_reference_count(provider), 1);
    assert_eq!(queue.purge_provider(provider), 1);
    assert_eq!(queue.provider_reference_count(provider), 0);
    assert!(!queue.references_provider(provider));
    let request_provider = OwnedShellIntent::Compose(OwnedCompositionIntent {
        source: screen,
        intent: CompositionIntent::Request {
            surface: surface(provider, 701),
            input: OverlayInput::Passive,
            lifetime: OverlayLifetime::Transient,
            rank: 1,
        },
    });
    queue.push(request_provider).unwrap();
    assert_eq!(queue.provider_reference_count(provider), 1);
    assert_eq!(queue.purge_provider(provider), 1);
    assert_eq!(queue.pop(), None);
}

#[test]
fn phase_one_capacity_budget_is_bounded_and_pointer_free() {
    assert_eq!(PROVIDER_CAPACITY, 8);
    assert_eq!(SURFACE_REGISTRY_CAPACITY, 16);
    assert_eq!(NAVIGATION_STACK_CAPACITY, 8);
    assert_eq!(LIVE_OVERLAY_CAPACITY, 4);
    assert_eq!(MODAL_QUEUE_CAPACITY, 4);
    assert_eq!(SHELL_INTENT_QUEUE_CAPACITY, 8);
    assert_eq!(RETAINED_MODEL_CAPACITY, 0);
    assert_eq!(FUTURE_RETAINED_MODEL_REFERENCE_CEILING, 4);
    assert_eq!(DEFAULT_SHELL_MODEL_BYTES, size_of::<DefaultShellModel>());
    let host_model_bytes = core::hint::black_box(DEFAULT_SHELL_MODEL_BYTES);
    assert!(host_model_bytes < 1_280);
    if size_of::<usize>() == 8 {
        assert_eq!(host_model_bytes, 1_048);
    }
    assert!(size_of::<CompositionReferences<4, 4>>() <= 256);
    assert!(size_of::<OwnedShellIntent>() <= 40);
    if size_of::<usize>() == 8 {
        assert_eq!(size_of::<CompositionReferences<4, 4>>(), 240);
    }
}
