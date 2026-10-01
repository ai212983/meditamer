//! The serial-driven UI cycle step and the provider-fixture harness it exercises.

use super::*;
use crate::firmware::types::UiCycleTarget;

#[cfg(feature = "ui-provider-fixture")]
use shell::types::ProviderToken;

impl Backend {
    pub(crate) fn cycle_step(
        &mut self,
        display: &mut InkplateDriver,
        target: UiCycleTarget,
    ) -> Result<DirtyArea, UiCycleStepError> {
        if self.coordinator.is_faulted() {
            return Err(UiCycleStepError::NavigationFault);
        }
        if self.coordinator.shell().active_modal().is_some() {
            return Err(UiCycleStepError::Busy);
        }
        if !self.active_surface_is_renderable() {
            return Err(UiCycleStepError::NavigationFault);
        }

        let current = self.coordinator.shell().active().surface;
        let (intent, expected) = match target {
            UiCycleTarget::AnalogClock => (
                NavIntent::Launch(self.surfaces.analog_clock),
                self.surfaces.analog_clock,
            ),
            UiCycleTarget::AmbientView if current == self.surfaces.launcher => (
                NavIntent::Launch(self.surfaces.ambient_view),
                self.surfaces.ambient_view,
            ),
            UiCycleTarget::Next
                if current == self.surfaces.home || current == self.surfaces.ambient_view =>
            {
                (
                    NavIntent::OpenLauncher(self.surfaces.launcher),
                    self.surfaces.launcher,
                )
            }
            UiCycleTarget::Next if current == self.surfaces.launcher => (
                NavIntent::Launch(self.surfaces.diagnostics),
                self.surfaces.diagnostics,
            ),
            UiCycleTarget::Next
                if current == self.surfaces.diagnostics
                    || current == self.surfaces.analog_clock =>
            {
                (NavIntent::Home, self.surfaces.home)
            }
            _ => return Err(UiCycleStepError::NavigationFault),
        };

        self.coordinator
            .queue_intent(OwnedNavIntent {
                source: self.coordinator.shell().active_instance(),
                intent,
            })
            .map_err(|_| UiCycleStepError::Busy)?;
        let dirty = self.render_with(display, |_| {});
        if self.coordinator.is_faulted()
            || self.coordinator.shell().active().surface != expected
            || !self.active_surface_is_renderable()
        {
            return Err(UiCycleStepError::NavigationFault);
        }
        dirty.ok_or(UiCycleStepError::NoDirty)
    }

    #[cfg(feature = "ui-provider-fixture")]
    pub(crate) fn provider_fixture_step(
        &mut self,
        display: &mut InkplateDriver,
    ) -> Result<DirtyArea, UiCycleStepError> {
        if self.fixture_blocked()
            || matches!(
                self.provider_fixture_state,
                ProviderFixtureState::Detaching(_)
            )
            || !self.active_surface_is_renderable()
        {
            return Err(UiCycleStepError::NavigationFault);
        }

        self.reregister_removed_provider()?;
        let dirty = self.advance_fixture_route(display)?;

        if self.fixture_blocked()
            || self.coordinator.navigation_faulted()
            || !self.active_surface_is_renderable()
        {
            return Err(UiCycleStepError::NavigationFault);
        }
        dirty.ok_or(UiCycleStepError::NoDirty)
    }

    /// Faults that stop the fixture regardless of which phase it is in.
    #[cfg(feature = "ui-provider-fixture")]
    fn fixture_blocked(&self) -> bool {
        self.coordinator.composition_faulted()
            || self.coordinator.lifecycle_audit_faulted()
            || self.coordinator.cleanup_blocked_screen().is_some()
            || self.coordinator.overlay_cleanup_blocked_len() != 0
    }

    /// After a removal the fixture provider is registered again from home.
    #[cfg(feature = "ui-provider-fixture")]
    fn reregister_removed_provider(&mut self) -> Result<(), UiCycleStepError> {
        if !matches!(self.provider_fixture_state, ProviderFixtureState::Removed) {
            return Ok(());
        }
        if self.coordinator.shell().active().surface != self.surfaces.home {
            return Err(UiCycleStepError::NavigationFault);
        }
        let owner = self
            .coordinator
            .register_provider(PROVIDER_FIXTURE_ID, &PROVIDER_FIXTURE_SURFACES)
            .map_err(|_| UiCycleStepError::NavigationFault)?;
        self.surfaces.provider_fixture = SurfaceRef::new(owner, PROVIDER_FIXTURE_ROOT_ID.0);
        self.surfaces.provider_overlay = SurfaceRef::new(owner, PROVIDER_FIXTURE_OVERLAY_ID.0);
        self.provider_fixture_state = ProviderFixtureState::Registered(owner);
        Ok(())
    }

    /// Drives one step of the fixture route: home to launcher to provider, then
    /// through the modal stack to the removal request.
    #[cfg(feature = "ui-provider-fixture")]
    fn advance_fixture_route(
        &mut self,
        display: &mut InkplateDriver,
    ) -> Result<Option<DirtyArea>, UiCycleStepError> {
        let current = self.coordinator.shell().active().surface;
        if current == self.surfaces.home {
            self.queue_fixture_navigation(NavIntent::OpenLauncher(self.surfaces.launcher))?;
            return Ok(self.render_with(display, |_| {}));
        }
        if current == self.surfaces.launcher {
            self.queue_fixture_navigation(NavIntent::Launch(self.surfaces.provider_fixture))?;
            return Ok(self.render_with(display, |_| {}));
        }
        if current != self.surfaces.provider_fixture {
            return Err(UiCycleStepError::NavigationFault);
        }

        match self.coordinator.shell().active_modal() {
            None => {
                let owned = self.fixture_modal_request(self.surfaces.provider_overlay, 1);
                Ok(self.render_with(display, |backend| backend.drain_composition_intent(owned)))
            }
            Some(active) if active.token.surface == self.surfaces.provider_overlay => {
                // A protected base modal replaces the live provider modal. The
                // request remains owned by this provider generation.
                let owned = self.fixture_modal_request(self.surfaces.confirm, 4);
                Ok(self.render_with(display, |backend| backend.drain_composition_intent(owned)))
            }
            Some(active) if active.token.surface == self.surfaces.confirm => {
                let ProviderFixtureState::Registered(owner) = self.provider_fixture_state else {
                    return Err(UiCycleStepError::NavigationFault);
                };
                if active.request_owner != owner || self.coordinator.shell().queued_modal_len() != 0
                {
                    return Err(UiCycleStepError::NavigationFault);
                }
                Ok(self.stage_provider_removal(display, owner))
            }
            Some(_) => Err(UiCycleStepError::NavigationFault),
        }
    }

    /// Re-queues the provider modal, checks the callback seam observed it, and
    /// then runs the removal. Faults are recorded on the backend rather than
    /// returned, because the render pass owns the transition.
    #[cfg(feature = "ui-provider-fixture")]
    fn stage_provider_removal(
        &mut self,
        display: &mut InkplateDriver,
        owner: ProviderToken,
    ) -> Option<DirtyArea> {
        let owned = self.fixture_modal_request(self.surfaces.provider_overlay, 1);
        self.render_with(display, move |backend| {
            backend.drain_composition_intent(owned);
            let queued = backend.coordinator.shell().queued_modal(0);
            if backend.coordinator.shell().queued_modal_len() != 1
                || queued.is_none_or(|queued| {
                    queued.token.surface != backend.surfaces.provider_overlay
                        || queued.request_owner != owner
                })
            {
                backend.coordinator.set_navigation_faulted(true);
                backend.coordinator.set_composition_faulted(true);
                console::println!(
                    "UI_PROVIDER_REMOVE state=fault stage=queued_owner owner={:?}",
                    owner,
                );
                return;
            }
            console::println!(
                "UI_PROVIDER_REMOVE state=staged owner={:?} provider_requested_base_live=true provider_modal_queued=true",
                owner,
            );
            if intent_bridge::queued_provider_action_count(owner) != 0
                || !backend.send_provider_remove_clicked()
                || intent_bridge::queued_provider_action_count(owner) != 1
            {
                intent_bridge::purge_provider(owner);
                backend.coordinator.set_navigation_faulted(true);
                backend.coordinator.set_composition_faulted(true);
                console::println!(
                    "UI_PROVIDER_REMOVE state=fault stage=callback_probe owner={:?}",
                    owner,
                );
                return;
            }
            backend.execute_provider_removal_fixture();
        })
    }

    #[cfg(feature = "ui-provider-fixture")]
    fn queue_fixture_navigation(&mut self, intent: NavIntent) -> Result<(), UiCycleStepError> {
        self.coordinator
            .queue_intent(OwnedNavIntent {
                source: self.coordinator.shell().active_instance(),
                intent,
            })
            .map_err(|_| UiCycleStepError::Busy)
    }

    #[cfg(feature = "ui-provider-fixture")]
    fn fixture_modal_request(&self, surface: SurfaceRef, rank: u8) -> OwnedCompositionIntent {
        OwnedCompositionIntent {
            source: self.coordinator.shell().active_instance(),
            intent: CompositionIntent::Request {
                surface,
                input: OverlayInput::Modal,
                lifetime: OverlayLifetime::Sticky,
                rank,
            },
        }
    }

    #[cfg(feature = "ui-provider-fixture")]
    fn send_provider_remove_clicked(&self) -> bool {
        let ui_token = &self.ui_token;
        self.coordinator.active_screen().is_some_and(|active| {
            matches!(
                &active.model,
                SurfaceModel::ProviderFixture(screen) if screen.send_remove_clicked(ui_token)
            )
        })
    }

    /// Drives provider removal through the coordinator. The fixture checks
    /// active-surface alignment first because the dispatch method trusts its
    /// `active_screen` unconditionally.
    ///
    /// The callback-route purge must run *before* dispatching the removal,
    /// not after (found live on hardware: dispatching first left `purged`
    /// at 0, failing the audit on every run). `dispatch_provider_removal`
    /// commits atomically with no caller hook mid-commit, so there is no
    /// point *between* the shell commit and the origin's teardown to run it
    /// at -- and the origin's own
    /// teardown (`ActiveSurface::destroy`'s existing `intent_bridge::
    /// purge_instance` call, unconditional for every surface) purges the
    /// exact same still-queued "remove clicked" callback action this fixture
    /// synthesized (it is sourced from the origin's own instance token),
    /// just via a different call. Purging here first, while dispatch hasn't
    /// run yet and the action is still guaranteed present, is what lets this
    /// count mean anything; purging after finds it already gone.
    #[cfg(feature = "ui-provider-fixture")]
    pub(super) fn execute_provider_removal_fixture(&mut self) {
        let ProviderFixtureState::Registered(owner) = self.provider_fixture_state else {
            self.coordinator.set_navigation_faulted(true);
            return;
        };
        if self.coordinator.is_faulted() {
            self.coordinator.set_navigation_faulted(true);
            return;
        }
        let aligned = self.coordinator.active_screen().is_some_and(|active| {
            active.token == self.coordinator.shell().active_instance()
                && active.token.surface.owner == owner
        });
        if !aligned {
            self.coordinator.set_navigation_faulted(true);
            console::println!(
                "UI_PROVIDER_REMOVE state=fault stage=alignment owner={:?}",
                owner,
            );
            return;
        }
        let purged = intent_bridge::purge_provider(owner);
        if purged != 1 {
            self.coordinator.set_navigation_faulted(true);
            self.coordinator.set_lifecycle_audit_faulted(true);
            console::println!(
                "UI_PROVIDER_REMOVE state=audit_failed stage=callback_purge owner={:?} expected=1 actual={}",
                owner,
                purged,
            );
            return;
        }
        let mut overlay_runtime = self.overlay_runtime();
        let mut screen_runtime = lvgl_surface_runtime(
            self.surfaces,
            &self.apps,
            &self.catalogue,
            self.settings.current(),
            self.ui_token.clone(),
            Instant::now().as_micros(),
            &mut self.analog_clock_shared,
            &mut self.ambient_shared,
        );
        let result = self.coordinator.dispatch_provider_removal(
            owner,
            &mut screen_runtime,
            &mut overlay_runtime,
        );
        self.sync_overlay_visibility_for_active_surface();
        self.sync_exclusive_capture();
        match result {
            Err(error) => {
                self.coordinator.set_navigation_faulted(true);
                console::println!(
                    "UI_PROVIDER_REMOVE state=rejected stage=prepare owner={:?} error={:?}",
                    owner,
                    error,
                );
            }
            Ok(ProviderRemovalDispatchOutcome::Rejected) => {
                console::println!(
                    "UI_PROVIDER_REMOVE state=rolled_back stage=prepare owner={:?}",
                    owner,
                );
            }
            Ok(ProviderRemovalDispatchOutcome::OverlayRuntimeMisaligned) => {
                console::println!(
                    "UI_PROVIDER_REMOVE state=fault stage=overlay_alignment owner={:?}",
                    owner,
                );
            }
            Ok(ProviderRemovalDispatchOutcome::Detached(pending)) => {
                console::println!(
                    "UI_PROVIDER_REMOVE state=detached owner={:?} callback_actions_purged={}",
                    owner,
                    purged,
                );
                self.provider_fixture_state = ProviderFixtureState::Detaching(pending);
                self.clear_provider_fixture_refs();
                self.try_finalize_provider_fixture_removal();
            }
        }
    }

    #[cfg(feature = "ui-provider-fixture")]
    pub(super) fn try_finalize_provider_fixture_removal(&mut self) {
        let ProviderFixtureState::Detaching(pending) = &self.provider_fixture_state else {
            return;
        };
        let owner = pending.owner();
        // Finalization must not consume a fault owned by another transition.
        if self.coordinator.is_faulted() {
            return;
        }
        let runtime_references = self
            .coordinator
            .active_screen()
            .is_some_and(|active| active.token.surface.owner == owner)
            || self
                .coordinator
                .cleanup_blocked_screen()
                .is_some_and(|active| active.token.surface.owner == owner)
            || self
                .coordinator
                .live_overlays()
                .chain(self.coordinator.overlay_cleanup_blocked())
                .any(|overlay| overlay.references_provider(owner));
        let callback_references = intent_bridge::references_provider(owner);
        let integrity_ok = self
            .ui_token
            .memory_snapshot()
            .is_ok_and(|snapshot| snapshot.integrity_ok);
        let shell_aligned = self.active_surface_is_renderable();
        if runtime_references || callback_references || !integrity_ok || !shell_aligned {
            self.coordinator.set_navigation_faulted(true);
            self.coordinator.set_lifecycle_audit_faulted(true);
            console::println!(
                "UI_PROVIDER_REMOVE state=audit_failed owner={:?} runtime_refs={} callback_refs={} integrity_ok={} shell_aligned={}",
                owner,
                runtime_references,
                callback_references,
                integrity_ok,
                shell_aligned,
            );
            return;
        }
        let finalized = self
            .coordinator
            .finalize_provider_removal(pending, ProviderRuntimeAudit::verified(owner));
        match finalized {
            Ok(purge) => {
                self.provider_fixture_state = ProviderFixtureState::Removed;
                console::println!(
                    "UI_PROVIDER_REMOVE state=finalized owner={:?} definitions={} overlays={} queued={}",
                    owner,
                    purge.definitions,
                    purge.composition.live_overlays,
                    purge.composition.queued_modals,
                );
                self.log_lifecycle_checkpoint("provider_finalized", 0);
            }
            Err(error) => {
                self.coordinator.set_navigation_faulted(true);
                self.coordinator.set_lifecycle_audit_faulted(true);
                console::println!(
                    "UI_PROVIDER_REMOVE state=audit_failed owner={:?} error={:?}",
                    owner,
                    error,
                );
            }
        }
    }

    #[cfg(feature = "ui-provider-fixture")]
    fn clear_provider_fixture_refs(&mut self) {
        self.surfaces.provider_fixture = self.surfaces.home;
        self.surfaces.provider_overlay = self.surfaces.confirm;
    }
}
