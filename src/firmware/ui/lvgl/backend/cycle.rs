//! The serial-driven UI cycle step and the provider-fixture harness it exercises.

use super::*;

#[cfg(feature = "ui-provider-fixture")]
use shell::types::ProviderToken;

impl Backend {
    pub(crate) fn cycle_step(
        &mut self,
        display: &mut InkplateDriver,
    ) -> Result<DirtyArea, UiCycleStepError> {
        if self.navigation_faulted
            || self.composition_faulted
            || self.cleanup_blocked.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
        {
            return Err(UiCycleStepError::NavigationFault);
        }
        if self.shell.active_modal().is_some() {
            return Err(UiCycleStepError::Busy);
        }
        if !self.active_surface_is_renderable() {
            return Err(UiCycleStepError::NavigationFault);
        }

        let current = self.shell.active().surface;
        let (intent, expected) = if current == self.surfaces.home {
            (
                NavIntent::OpenLauncher(self.surfaces.launcher),
                self.surfaces.launcher,
            )
        } else if current == self.surfaces.launcher {
            (
                NavIntent::Launch(self.surfaces.diagnostics),
                self.surfaces.diagnostics,
            )
        } else if current == self.surfaces.diagnostics {
            (NavIntent::Home, self.surfaces.home)
        } else {
            return Err(UiCycleStepError::NavigationFault);
        };

        self.shell
            .queue_intent(OwnedNavIntent {
                source: self.shell.active_instance(),
                intent,
            })
            .map_err(|_| UiCycleStepError::Busy)?;
        let dirty = self.render_with(display, |_| {});
        if self.navigation_faulted
            || self.composition_faulted
            || self.cleanup_blocked.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
            || self.shell.active().surface != expected
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

        if self.fixture_blocked() || self.navigation_faulted || !self.active_surface_is_renderable()
        {
            return Err(UiCycleStepError::NavigationFault);
        }
        dirty.ok_or(UiCycleStepError::NoDirty)
    }

    /// Faults that stop the fixture regardless of which phase it is in.
    #[cfg(feature = "ui-provider-fixture")]
    fn fixture_blocked(&self) -> bool {
        self.composition_faulted
            || self.lifecycle_audit_faulted
            || self.cleanup_blocked.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
    }

    /// After a removal the fixture provider is registered again from home.
    #[cfg(feature = "ui-provider-fixture")]
    fn reregister_removed_provider(&mut self) -> Result<(), UiCycleStepError> {
        if !matches!(self.provider_fixture_state, ProviderFixtureState::Removed) {
            return Ok(());
        }
        if self.shell.active().surface != self.surfaces.home {
            return Err(UiCycleStepError::NavigationFault);
        }
        let owner = self
            .shell
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
        let current = self.shell.active().surface;
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

        match self.shell.active_modal() {
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
                if active.request_owner != owner || self.shell.queued_modal_len() != 0 {
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
            let queued = backend.shell.queued_modal(0);
            if backend.shell.queued_modal_len() != 1
                || queued.is_none_or(|queued| {
                    queued.token.surface != backend.surfaces.provider_overlay
                        || queued.request_owner != owner
                })
            {
                backend.navigation_faulted = true;
                backend.composition_faulted = true;
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
                backend.navigation_faulted = true;
                backend.composition_faulted = true;
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
        self.shell
            .queue_intent(OwnedNavIntent {
                source: self.shell.active_instance(),
                intent,
            })
            .map_err(|_| UiCycleStepError::Busy)
    }

    #[cfg(feature = "ui-provider-fixture")]
    fn fixture_modal_request(&self, surface: SurfaceRef, rank: u8) -> OwnedCompositionIntent {
        OwnedCompositionIntent {
            source: self.shell.active_instance(),
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
        self.active.as_ref().is_some_and(|active| {
            matches!(
                &active.model,
                SurfaceModel::ProviderFixture(screen) if screen.send_remove_clicked()
            )
        })
    }

    #[cfg(feature = "ui-provider-fixture")]
    pub(super) fn execute_provider_removal_fixture(&mut self) {
        let ProviderFixtureState::Registered(owner) = self.provider_fixture_state else {
            self.navigation_faulted = true;
            return;
        };
        if self.navigation_faulted
            || self.composition_faulted
            || self.lifecycle_audit_faulted
            || self.cleanup_blocked.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
        {
            self.navigation_faulted = true;
            return;
        }
        let plan = match self.shell.prepare_provider_removal(owner) {
            Ok(plan) => plan,
            Err(error) => {
                self.navigation_faulted = true;
                console::println!(
                    "UI_PROVIDER_REMOVE state=rejected stage=prepare owner={:?} error={:?}",
                    owner,
                    error,
                );
                return;
            }
        };
        let Some(fallback) = plan.fallback_transition() else {
            self.navigation_faulted = true;
            console::println!(
                "UI_PROVIDER_REMOVE state=rejected stage=fallback owner={:?}",
                owner,
            );
            return;
        };
        if self.active.as_ref().is_none_or(|active| {
            active.frame != fallback.origin
                || active.token != self.shell.active_instance()
                || active.token.surface.owner != owner
        }) {
            self.navigation_faulted = true;
            console::println!(
                "UI_PROVIDER_REMOVE state=fault stage=alignment owner={:?}",
                owner,
            );
            return;
        }
        let mut entering_overlays =
            match self.stage_overlay_entries(plan.composition_delta().enter_live()) {
                Some(candidates) => candidates,
                None => {
                    console::println!(
                        "UI_PROVIDER_REMOVE state=rolled_back stage=overlay_entry owner={:?}",
                        owner,
                    );
                    return;
                }
            };
        let mut departing_overlays = Vec::<SurfaceInstanceToken, LIVE_OVERLAY_CAPACITY>::new();
        for instance in plan.composition_delta().leave_live() {
            departing_overlays
                .push(instance.token)
                .expect("the provider delta is bounded by live overlay capacity");
        }
        if !self.stage_overlay_departures(&departing_overlays) {
            self.destroy_uncommitted_overlays(&mut entering_overlays);
            self.navigation_faulted = true;
            self.composition_faulted = true;
            console::println!(
                "UI_PROVIDER_REMOVE state=fault stage=overlay_alignment owner={:?}",
                owner,
            );
            return;
        }
        let Some(origin) = self.active.take() else {
            self.restore_overlay_departures(&departing_overlays);
            self.destroy_uncommitted_overlays(&mut entering_overlays);
            self.navigation_faulted = true;
            return;
        };
        let transition_started_us = Instant::now().as_micros();
        let mut runtime = LvglSurfaceRuntime {
            surfaces: self.surfaces,
            catalogue: &self.catalogue,
            settings: self.settings.current(),
            transition_started_us,
        };
        let result = execute_transition(
            &mut runtime,
            origin,
            fallback.destination,
            fallback.destination_instance,
            || {
                let pending = self.shell.commit_provider_detach(plan)?;
                let purged = intent_bridge::purge_provider(owner);
                if purged != 1 {
                    self.navigation_faulted = true;
                    self.lifecycle_audit_faulted = true;
                    console::println!(
                        "UI_PROVIDER_REMOVE state=audit_failed stage=callback_purge owner={:?} expected=1 actual={}",
                        owner,
                        purged,
                    );
                }
                console::println!(
                    "UI_PROVIDER_REMOVE state=detached owner={:?} callback_actions_purged={}",
                    owner,
                    purged,
                );
                Ok::<_, shell::model::ProviderRemovalError>(pending)
            },
        );
        match result {
            TransitionResult::Committed {
                active,
                outcome: pending,
            } => {
                self.active = Some(active);
                self.provider_fixture_state = ProviderFixtureState::Detaching(pending);
                self.clear_provider_fixture_refs();
                self.complete_overlay_departures(&departing_overlays);
                self.activate_overlay_entries(&mut entering_overlays);
                self.try_finalize_provider_fixture_removal();
            }
            TransitionResult::RolledBack {
                active,
                cleanup_blocked,
                cleanup_audit_failed,
                reason,
            } => {
                self.lifecycle_audit_faulted |= cleanup_audit_failed;
                self.active = Some(active);
                self.cleanup_blocked = cleanup_blocked;
                self.restore_overlay_departures(&departing_overlays);
                self.destroy_uncommitted_overlays(&mut entering_overlays);
                self.navigation_faulted = self.cleanup_blocked.is_some()
                    || cleanup_audit_failed
                    || !self.active_surface_is_renderable();
                console::println!(
                    "UI_PROVIDER_REMOVE state=rolled_back owner={:?} reason={:?}",
                    owner,
                    reason,
                );
            }
            TransitionResult::FaultedAfterCommit {
                active,
                cleanup_blocked,
                cleanup_audit_failed,
                outcome: pending,
                reason,
            } => {
                self.lifecycle_audit_faulted |= cleanup_audit_failed;
                self.active = Some(active);
                self.cleanup_blocked = cleanup_blocked;
                self.provider_fixture_state = ProviderFixtureState::Detaching(pending);
                self.clear_provider_fixture_refs();
                self.complete_overlay_departures(&departing_overlays);
                self.activate_overlay_entries(&mut entering_overlays);
                self.navigation_faulted = true;
                console::println!(
                    "UI_PROVIDER_REMOVE state=cleanup_blocked owner={:?} reason={:?}",
                    owner,
                    reason,
                );
            }
        }
    }

    #[cfg(feature = "ui-provider-fixture")]
    pub(super) fn try_finalize_provider_fixture_removal(&mut self) {
        let ProviderFixtureState::Detaching(pending) = &self.provider_fixture_state else {
            return;
        };
        let owner = pending.owner();
        if self.cleanup_blocked.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
            || self.composition_faulted
            || self.lifecycle_audit_faulted
        {
            return;
        }
        let runtime_references = self
            .active
            .as_ref()
            .is_some_and(|active| active.token.surface.owner == owner)
            || self
                .cleanup_blocked
                .as_ref()
                .is_some_and(|active| active.token.surface.owner == owner)
            || self
                .overlays
                .iter()
                .chain(self.overlay_cleanup_blocked.iter())
                .any(|overlay| overlay.references_provider(owner));
        let callback_references = intent_bridge::references_provider(owner);
        let integrity_ok = unsafe { lv::lv_mem_test() == lv::lv_result_t_LV_RESULT_OK };
        let shell_aligned = self.active_surface_is_renderable();
        if runtime_references || callback_references || !integrity_ok || !shell_aligned {
            self.navigation_faulted = true;
            self.lifecycle_audit_faulted = true;
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
            .shell
            .finalize_provider_removal(pending, ProviderRuntimeAudit::verified(owner));
        match finalized {
            Ok(purge) => {
                self.provider_fixture_state = ProviderFixtureState::Removed;
                self.navigation_faulted = false;
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
                self.navigation_faulted = true;
                self.lifecycle_audit_faulted = true;
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
