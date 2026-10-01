//! Overlay lifecycle: composition intents, visibility, and deferred cleanup.

use super::*;

impl Backend {
    fn active_surface_owns_screen_exclusively(&self) -> bool {
        self.coordinator.shell().active().surface == self.surfaces.ambient_view
    }

    /// Re-derives every live overlay's visibility from the current active
    /// surface. Needed on top of what [`shell::coordinator::UiCoordinator`]'s
    /// own dispatch already shows/hides for the overlays actually entering
    /// or leaving a transition: an overlay that was already live before a
    /// navigation that changes *only* which surface owns the screen
    /// exclusively (Home to Ambient View, say) is untouched by that
    /// transition's own composition delta and needs this separate sweep.
    pub(super) fn sync_overlay_visibility_for_active_surface(&self) {
        let exclusive = self.active_surface_owns_screen_exclusively();
        let ui_token = &self.ui_token;
        for overlay in self.coordinator.live_overlays() {
            // A clock overlay staged for removal (ADR-0025 item 5) stays
            // hidden regardless of this sweep's normal exclusive/modal rule
            // until the staged Clean scan commits its teardown -- otherwise
            // this unconditional per-frame sweep would re-show it.
            if self.clock_removal.staged() == Some(overlay.instance().token) {
                overlay.hide(ui_token);
                continue;
            }
            if exclusive && !overlay.is_modal() {
                overlay.hide(ui_token);
            } else {
                overlay.show(ui_token);
            }
        }
    }

    /// Drops the system layer's click capture once no modal remains live,
    /// queued, or stuck in a cleanup fault. `UiCoordinator` itself only ever
    /// grants capture (on a modal's entry, from `LvglOverlayRuntime::enter`
    /// completing) -- there is exactly one point where every modal source
    /// converges to "none remain," so revocation lives here instead of
    /// being threaded through every dispatch outcome.
    pub(super) fn sync_exclusive_capture(&self) {
        let no_modal_remains = self.coordinator.shell().active_modal().is_none()
            && self
                .coordinator
                .live_overlays()
                .all(|overlay| !overlay.is_modal())
            && self
                .coordinator
                .overlay_cleanup_blocked()
                .all(|overlay| !overlay.is_modal());
        if no_modal_remains {
            let _ = self.ui_token.set_system_layer_capture(false);
        }
    }

    /// `false` once a removal is requested or staged (ADR-0025 item 5): a
    /// tap or a fresh timeout check must not resurrect an overlay that is
    /// already queued for removal, hidden, or awaiting its Clean scan.
    pub(super) fn clock_overlay_active(&self) -> bool {
        self.clock_removal.staged().is_none()
            && self.clock_removal.requested().is_none()
            && self
                .coordinator
                .shell()
                .active_modal()
                .is_some_and(|instance| instance.token.surface == self.surfaces.clock_overlay)
            && self
                .coordinator
                .live_overlays()
                .any(|overlay| overlay.instance().token.surface == self.surfaces.clock_overlay)
    }

    /// The live clock overlay's own token, if one is currently admitted --
    /// used both to tag a deferred Fast render (ADR-0025 item 5) and to
    /// revalidate a retry against it.
    pub(crate) fn clock_overlay_token(&self) -> Option<SurfaceInstanceToken> {
        self.coordinator
            .live_overlays()
            .find(|overlay| overlay.instance().token.surface == self.surfaces.clock_overlay)
            .map(|overlay| overlay.instance().token)
    }

    pub(super) fn clock_overlay_timeout_elapsed(&self, now_ms: u64) -> bool {
        self.coordinator
            .live_overlays()
            .find(|overlay| overlay.instance().token.surface == self.surfaces.clock_overlay)
            .is_some_and(|overlay| overlay.clock_timeout_elapsed(now_ms))
    }

    pub(super) fn show_or_refresh_clock_overlay(
        &mut self,
        local_epoch_seconds: u32,
        now_ms: u64,
    ) -> bool {
        if let Some(active) = self.coordinator.shell().active_modal() {
            if active.token.surface != self.surfaces.clock_overlay {
                return false;
            }
            let ui_token = self.ui_token.clone();
            return self
                .coordinator
                .live_overlays_mut()
                .find(|overlay| overlay.instance().token == active.token)
                .and_then(|overlay| overlay.update_clock(&ui_token, local_epoch_seconds, now_ms))
                .is_some_and(|result| result.unwrap_or(false));
        }

        let owned = OwnedCompositionIntent {
            source: self.coordinator.shell().active_instance(),
            intent: CompositionIntent::Request {
                surface: self.surfaces.clock_overlay,
                input: OverlayInput::Modal,
                lifetime: OverlayLifetime::Transient,
                rank: 4,
            },
        };
        let result = {
            let mut overlay_runtime = self.overlay_runtime();
            self.coordinator
                .dispatch_composition(owned, &mut overlay_runtime)
        };
        self.sync_overlay_visibility_for_active_surface();
        self.sync_exclusive_capture();
        let Ok(CompositionDispatchOutcome::Admitted(OverlayAdmission::Active(instance))) = result
        else {
            return false;
        };
        let ui_token = self.ui_token.clone();
        let initialized = self
            .coordinator
            .live_overlays_mut()
            .find(|overlay| overlay.instance().token == instance.token)
            .and_then(|overlay| overlay.update_clock(&ui_token, local_epoch_seconds, now_ms))
            .is_some_and(|result| result.is_ok());
        if !initialized {
            let _ = self.remove_clock_overlay(instance.token);
        }
        initialized
    }

    /// Records a removal request without touching coordinator, widget,
    /// capture, or framebuffer state (ADR-0025 item 5): the request-tap or
    /// timeout may land while Clean is prohibited, so it queues here rather
    /// than mutating anything. Always supersedes any not-yet-presented Fast
    /// clock work -- a queued removal wins over unpresented Fast work
    /// already rendered for the same overlay.
    pub(super) fn request_clock_removal(&mut self, token: SurfaceInstanceToken) {
        self.clock_removal.request(token);
        self.pending_fast_clock = PendingFastClock::None;
    }

    /// A removal requested via `request_clock_removal` but not yet staged
    /// (hidden). Presentation drives this to the staged state, inside a
    /// normal LVGL publish frame, once Clean is allowed.
    pub(crate) fn clock_removal_requested(&self) -> Option<SurfaceInstanceToken> {
        self.clock_removal.requested()
    }

    /// Directly stages the clock overlay for removal (ADR-0025 item 5):
    /// hides its widget without going through composition dispatch, so
    /// shell ownership and modal capture are retained until a Clean scan
    /// commits the actual teardown. Callers already know Clean is allowed
    /// (a deferred timeout resolves this way once it becomes executable; a
    /// promoted request goes through `promote_requested_clock_removal`
    /// instead, inside a render frame).
    pub(super) fn stage_clock_removal(&mut self, token: SurfaceInstanceToken) {
        if self.clock_removal.staged().is_none() {
            if let Some(overlay) = self
                .coordinator
                .live_overlays()
                .find(|overlay| overlay.instance().token == token)
            {
                overlay.hide(&self.ui_token);
            }
            self.clock_removal.stage(token);
        }
        self.pending_fast_clock = PendingFastClock::None;
    }

    pub(crate) fn clock_removal_pending(&self) -> Option<SurfaceInstanceToken> {
        self.clock_removal.staged()
    }

    fn screen_update_source_is_current(&self, source: SurfaceInstanceToken) -> bool {
        self.coordinator
            .active_screen()
            .is_some_and(|active| active.token == source)
            || self
                .coordinator
                .live_overlays()
                .any(|overlay| overlay.instance().token == source)
    }

    pub(super) const fn screen_update_slot(intent: ScreenUpdateIntent) -> usize {
        match intent {
            ScreenUpdateIntent::Fast => 0,
            ScreenUpdateIntent::Clean => 1,
        }
    }

    fn discard_stale_screen_updates(&mut self) {
        for index in 0..self.pending_screen_updates.len() {
            let stale = self.pending_screen_updates[index]
                .is_some_and(|pending| !self.screen_update_source_is_current(pending.source));
            if stale {
                self.pending_screen_updates[index] = None;
            }
        }
    }

    pub(crate) fn has_pending_screen_update(&mut self, intent: ScreenUpdateIntent) -> bool {
        self.discard_stale_screen_updates();
        self.pending_screen_updates[Self::screen_update_slot(intent)].is_some()
    }

    pub(crate) fn pending_screen_update_intent(&mut self) -> Option<ScreenUpdateIntent> {
        self.discard_stale_screen_updates();
        if self.pending_screen_updates[Self::screen_update_slot(ScreenUpdateIntent::Clean)]
            .is_some()
        {
            Some(ScreenUpdateIntent::Clean)
        } else if self.pending_screen_updates[Self::screen_update_slot(ScreenUpdateIntent::Fast)]
            .is_some()
        {
            Some(ScreenUpdateIntent::Fast)
        } else {
            None
        }
    }

    /// Pair a validated semantic request with the renderer damage produced
    /// by the frame that delivered it.
    pub(crate) fn take_pending_screen_update(
        &mut self,
        damage: Option<DirtyArea>,
    ) -> Option<ScreenUpdate<DirtyArea>> {
        let intent = self.pending_screen_update_intent()?;
        self.take_pending_screen_update_for(intent, damage)
    }

    pub(crate) fn take_pending_screen_update_for(
        &mut self,
        intent: ScreenUpdateIntent,
        damage: Option<DirtyArea>,
    ) -> Option<ScreenUpdate<DirtyArea>> {
        self.discard_stale_screen_updates();
        let request = self.pending_screen_updates[Self::screen_update_slot(intent)].take()?;
        if intent == ScreenUpdateIntent::Clean {
            self.pending_screen_updates = [None; 2];
        }
        Some(request.request.with_damage(damage))
    }

    /// Commits a staged removal after its Clean scan succeeded: destroys the
    /// overlay and releases capture exactly once. A teardown failure here is
    /// the existing retryable cleanup fault (`retry_blocked_cleanup`,
    /// already run every `drain_navigation`) -- it keeps input blocked and
    /// the overlay hidden on its own, so this clears the staging marker
    /// either way rather than tracking that retry twice.
    pub(crate) fn commit_clock_removal(&mut self) {
        if let Some(token) = self.clock_removal.take_staged() {
            let _ = self.remove_clock_overlay(token);
        }
    }

    fn remove_clock_overlay(&mut self, token: SurfaceInstanceToken) -> bool {
        let result = {
            let mut overlay_runtime = self.overlay_runtime();
            self.coordinator
                .dispatch_overlay_removal(token, &mut overlay_runtime)
        };
        self.sync_overlay_visibility_for_active_surface();
        self.sync_exclusive_capture();
        matches!(result, Ok(CompositionDispatchOutcome::Removed))
    }

    pub(super) fn drain_navigation(&mut self) {
        // Overlay runtime first (same borrow ordering as navigation.rs):
        // it only borrows `self` for the call, while the screen runtime
        // holds `&mut analog_clock_shared` and `&mut ambient_shared` across
        // the retry.
        let mut overlay_runtime = self.overlay_runtime();
        let mut screen_runtime = lvgl_surface_runtime(
            self.surfaces,
            &self.apps,
            &self.catalogue,
            self.settings.current(),
            self.ui_token.clone(),
            0,
            &mut self.analog_clock_shared,
            &mut self.ambient_shared,
        );
        self.coordinator
            .retry_blocked_cleanup(&mut screen_runtime, &mut overlay_runtime);
        // Reclaims any widget a construction failure parked because it
        // could not delete what it had already built
        // (`render::lvgl_adapter::park_orphaned_widget`) -- a no-op
        // fixed-capacity check when nothing is parked, run alongside the
        // screen/overlay-level retry above rather than on its own cadence.
        render::lvgl_adapter::retry_parked_widgets(&self.ui_token);
        self.sync_overlay_visibility_for_active_surface();
        self.sync_exclusive_capture();
        #[cfg(feature = "ui-provider-fixture")]
        self.try_finalize_provider_fixture_removal();
        if intent_bridge::take_overflowed() {
            console::println!("UI_NAV state=rejected reason=callback_queue_full");
        }
        while let Some(action) = intent_bridge::take_value_action() {
            self.drain_indexed_value_action(action);
        }
        while let Some(action) = intent_bridge::take_action() {
            self.drain_indexed_action(action);
        }
        while let Some(action) = intent_bridge::take_intent() {
            match action {
                OwnedShellIntent::Navigate(intent) => {
                    if intent.source.surface == self.surfaces.clock_overlay {
                        // Overlay-originated navigation is a validated
                        // shared-shell lifecycle transaction: only the
                        // current active transient modal may navigate, and
                        // `dispatch_navigation`'s own transient composition
                        // delta removes Clock while activating Launcher --
                        // Clock stays physically visible until that single
                        // Clean scan presents Launcher; Ambient is never
                        // presented between them.
                        let authorized = self
                            .coordinator
                            .shell()
                            .active_modal()
                            .is_some_and(|modal| modal.token == intent.source)
                            && self.clock_overlay_token() == Some(intent.source)
                            && intent.intent == NavIntent::OpenLauncher(self.surfaces.launcher);
                        if !authorized || self.coordinator.is_faulted() {
                            console::println!(
                                "UI_NAV state=rejected reason=stale_or_unauthorized source={:?} intent={:?}",
                                intent.source,
                                intent.intent,
                            );
                            continue;
                        }
                        if let Err(error) = self.coordinator.queue_intent(intent) {
                            console::println!(
                                "UI_NAV state=rejected reason=shell_queue error={:?}",
                                error
                            );
                        }
                        self.drain_shell_navigation();
                        continue;
                    }
                    if let Err(error) = self.coordinator.queue_intent(intent) {
                        console::println!(
                            "UI_NAV state=rejected reason=shell_queue error={:?}",
                            error
                        );
                    }
                    self.drain_shell_navigation();
                }
                OwnedShellIntent::Compose(intent) => self.drain_composition_intent(intent),
                // The sticky refresh-control overlay is this crate's only
                // current producer, and it always resolves to Clean with no
                // renderer damage (`OwnedRefreshIntent`'s conversion in
                // `shell::types`); a future producer's own declared intent
                // would need its damage threaded through from here too.
                OwnedShellIntent::ScreenUpdate(request) => {
                    let source_current = self.screen_update_source_is_current(request.source);
                    if !source_current || self.coordinator.is_faulted() {
                        console::println!(
                            "UI_SCREEN_UPDATE state=rejected reason=stale_or_blocked source={:?}",
                            request.source,
                        );
                        continue;
                    }
                    let slot = Self::screen_update_slot(request.request.intent);
                    self.pending_screen_updates[slot] = Some(request);
                }
                OwnedShellIntent::Configure(intent) => self.drain_settings_intent(intent),
            }
        }
        self.drain_shell_navigation();
    }

    fn drain_indexed_value_action(&mut self, action: intent_bridge::IndexedValueAction) {
        let source_current = self
            .coordinator
            .shell()
            .active_modal()
            .is_some_and(|instance| instance.token == action.source);
        if !source_current || self.coordinator.is_faulted() {
            console::println!(
                "UI_ACTION state=rejected reason=stale_or_blocked source={:?} index={} value={}",
                action.source,
                action.index,
                action.value,
            );
            return;
        }
        let ui_token = self.ui_token.clone();
        let effect = self
            .coordinator
            .live_overlays_mut()
            .find(|overlay| overlay.instance().token == action.source)
            .and_then(|overlay| overlay.apply_frontlight_value(&ui_token, action));
        let Some(effect) = effect else {
            console::println!(
                "UI_ACTION state=rejected reason=unsupported source={:?} index={} value={}",
                action.source,
                action.index,
                action.value,
            );
            return;
        };
        self.pending_frontlight_effect = Some(effect);
        if matches!(
            effect,
            FrontlightCalibrationEffect::Commit(_) | FrontlightCalibrationEffect::Cancel
        ) {
            self.drain_composition_intent(OwnedCompositionIntent {
                source: action.source,
                intent: CompositionIntent::DismissActiveModal,
            });
        }
    }

    fn drain_indexed_action(&mut self, action: intent_bridge::IndexedAction) {
        let source_current = self
            .coordinator
            .shell()
            .active_modal()
            .is_some_and(|instance| instance.token == action.source);
        if !source_current || self.coordinator.is_faulted() {
            console::println!(
                "UI_ACTION state=rejected reason=stale_or_blocked source={:?} index={}",
                action.source,
                action.index,
            );
            return;
        }
        let effect = self
            .coordinator
            .live_overlays_mut()
            .find(|overlay| overlay.instance().token == action.source)
            .and_then(|overlay| overlay.apply_frontlight_action(action.index));
        let Some(effect) = effect else {
            console::println!(
                "UI_ACTION state=rejected reason=unsupported source={:?} index={}",
                action.source,
                action.index,
            );
            return;
        };
        self.pending_frontlight_effect = Some(effect);
        if matches!(
            effect,
            FrontlightCalibrationEffect::Commit(_) | FrontlightCalibrationEffect::Cancel
        ) {
            self.drain_composition_intent(OwnedCompositionIntent {
                source: action.source,
                intent: CompositionIntent::DismissActiveModal,
            });
        }
    }

    pub(crate) fn show_frontlight_calibration(
        &mut self,
        display: &mut InkplateDriver,
        initial_level: u8,
    ) -> (bool, Option<DirtyArea>) {
        if self.coordinator.shell().active_modal().is_some() || self.coordinator.is_faulted() {
            return (false, None);
        }
        self.frontlight_calibration_initial = initial_level;
        let owned = OwnedCompositionIntent {
            source: self.coordinator.shell().active_instance(),
            intent: CompositionIntent::Request {
                surface: self.surfaces.frontlight_calibration,
                input: OverlayInput::Modal,
                lifetime: OverlayLifetime::Transient,
                rank: 5,
            },
        };
        let dirty = self.render_with(display, |backend| backend.drain_composition_intent(owned));
        (self.frontlight_calibration_active(), dirty)
    }

    pub(crate) fn frontlight_calibration_active(&self) -> bool {
        self.coordinator
            .shell()
            .active_modal()
            .is_some_and(|instance| instance.token.surface == self.surfaces.frontlight_calibration)
            && self.coordinator.live_overlays().any(|overlay| {
                overlay.instance().token.surface == self.surfaces.frontlight_calibration
            })
    }

    pub(crate) fn take_frontlight_calibration_effect(
        &mut self,
    ) -> Option<FrontlightCalibrationEffect> {
        self.pending_frontlight_effect.take()
    }

    pub(crate) fn frontlight_calibration_effect_pending(&self) -> bool {
        self.pending_frontlight_effect.is_some()
    }

    pub(crate) fn frontlight_preview_pending(&self) -> bool {
        matches!(
            self.pending_frontlight_effect,
            Some(FrontlightCalibrationEffect::Preview(_))
        )
    }

    fn drain_settings_intent(&mut self, owned: OwnedUiSettingsIntent) {
        if self
            .coordinator
            .active_screen()
            .is_none_or(|active| active.token != owned.source)
            || self.coordinator.is_faulted()
        {
            console::println!(
                "UI_SETTINGS state=rejected reason=stale_or_blocked source={:?}",
                owned.source,
            );
            return;
        }

        let now_ms = Instant::now().as_millis();
        let return_intent = match owned.intent {
            shell::settings::UiSettingsIntent::SelectAmbient(id) => {
                if !self
                    .catalogue
                    .entry_is_ready_for(id, CatalogueViewKind::AmbientPicker)
                {
                    console::println!(
                        "UI_SETTINGS state=rejected kind=ambient reason=unavailable id={:?}",
                        id,
                    );
                    return;
                }
                let changed = self.settings.select_ambient(id, now_ms);
                console::println!(
                    "UI_SETTINGS state={} kind=ambient id={:?}",
                    if changed { "changed" } else { "unchanged" },
                    id,
                );
                NavIntent::Home
            }
            shell::settings::UiSettingsIntent::ToggleOverlay(id) => {
                if id != REFRESH_CONTROL_ENTRY_ID
                    || !self
                        .catalogue
                        .entry_is_ready_for(id, CatalogueViewKind::OverlayToggles)
                {
                    console::println!(
                        "UI_SETTINGS state=rejected kind=overlay reason=unavailable id={:?}",
                        id,
                    );
                    return;
                }
                let enable = !self.settings.current().overlay_enabled(id);
                let lifecycle_ok = if enable {
                    self.install_base_overlay(
                        self.surfaces.sticky_status,
                        OverlayLifetime::Sticky,
                        2,
                        BaseOverlayKind::RefreshControl,
                    )
                    .is_ok()
                } else {
                    self.remove_settings_overlay(self.surfaces.sticky_status)
                };
                if !lifecycle_ok {
                    console::println!(
                        "UI_SETTINGS state=rejected kind=overlay reason=lifecycle id={:?}",
                        id,
                    );
                    return;
                }
                let applied = self.settings.toggle_overlay(id, now_ms);
                console::println!(
                    "UI_SETTINGS state=changed kind=overlay id={:?} enabled={}",
                    id,
                    applied.unwrap_or(enable),
                );
                NavIntent::Back
            }
        };

        if self
            .coordinator
            .queue_intent(OwnedNavIntent {
                source: self.coordinator.shell().active_instance(),
                intent: return_intent,
            })
            .is_err()
        {
            console::println!("UI_SETTINGS state=applied navigation=deferred");
            return;
        }
        self.drain_shell_navigation();
    }

    fn remove_settings_overlay(&mut self, surface: SurfaceRef) -> bool {
        let Some(token) = self
            .coordinator
            .live_overlays()
            .find(|overlay| overlay.instance().token.surface == surface)
            .map(|overlay| overlay.instance().token)
        else {
            return true;
        };
        let mut overlay_runtime = self.overlay_runtime();
        let result = self
            .coordinator
            .dispatch_overlay_removal(token, &mut overlay_runtime);
        self.sync_overlay_visibility_for_active_surface();
        self.sync_exclusive_capture();
        if let Err(error) = result {
            console::println!(
                "UI_COMPOSITION state=rejected reason=prepare token={:?} error={:?}",
                token,
                error,
            );
        }
        !self
            .coordinator
            .live_overlays()
            .any(|overlay| overlay.instance().token.surface == surface)
            && self.coordinator.overlay_cleanup_blocked_len() == 0
            && !self.coordinator.composition_faulted()
    }

    pub(super) fn drain_composition_intent(&mut self, owned: OwnedCompositionIntent) {
        if self.coordinator.is_faulted() {
            console::println!(
                "UI_COMPOSITION state=rejected reason=cleanup_blocked source={:?}",
                owned.source,
            );
            return;
        }
        let source = owned.source;
        let intent = owned.intent;
        if intent == CompositionIntent::DismissActiveModal
            && source.surface == self.surfaces.clock_overlay
        {
            let source_current = self
                .coordinator
                .shell()
                .active_modal()
                .is_some_and(|instance| instance.token == source)
                && self.clock_overlay_token() == Some(source);
            if !source_current {
                console::println!(
                    "UI_COMPOSITION state=rejected reason=stale_clock source={:?}",
                    source,
                );
                return;
            }
            // Clock removal is staged as a Clean UI transaction (ADR-0025
            // item 5): no coordinator, widget, capture, or framebuffer
            // mutation happens here, only a bookkeeping request. The display
            // layer hides the overlay inside a normal LVGL publish frame
            // once Clean is allowed, and commits teardown only after that
            // scan succeeds.
            self.request_clock_removal(source);
            console::println!("UI_COMPOSITION state=requested token={:?}", source);
            return;
        }
        let mut overlay_runtime = self.overlay_runtime();
        let result = self
            .coordinator
            .dispatch_composition(owned, &mut overlay_runtime);
        self.sync_overlay_visibility_for_active_surface();
        self.sync_exclusive_capture();
        match result {
            Err(error) => {
                console::println!(
                    "UI_COMPOSITION state=rejected reason=prepare source={:?} intent={:?} error={:?}",
                    source,
                    intent,
                    error,
                );
            }
            Ok(CompositionDispatchOutcome::Admitted(OverlayAdmission::Queued(instance))) => {
                console::println!("UI_COMPOSITION state=queued token={:?}", instance.token);
            }
            Ok(CompositionDispatchOutcome::Admitted(OverlayAdmission::Active(instance))) => {
                console::println!(
                    "UI_COMPOSITION state=active token={:?} input={:?}",
                    instance.token,
                    instance.input,
                );
            }
            Ok(CompositionDispatchOutcome::Removed) => {
                console::println!("UI_COMPOSITION state=dismissed");
            }
            Ok(CompositionDispatchOutcome::RolledBack(reason)) => {
                console::println!(
                    "UI_COMPOSITION state=rolled_back reason={:?} navigation_faulted={}",
                    reason,
                    self.coordinator.navigation_faulted(),
                );
            }
        }
    }
}
