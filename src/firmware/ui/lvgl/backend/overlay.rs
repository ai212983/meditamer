//! Overlay lifecycle: composition intents, entry, departure, and deferred cleanup.

use super::*;

impl Backend {
    fn active_surface_owns_screen_exclusively(&self) -> bool {
        self.shell.active().surface == self.surfaces.ambient_view
    }

    pub(super) fn sync_overlay_visibility_for_active_surface(&self) {
        let exclusive = self.active_surface_owns_screen_exclusively();
        for overlay in &self.overlays {
            if exclusive && !overlay.is_modal() {
                overlay.hide();
            } else {
                overlay.show();
            }
        }
    }

    pub(super) fn drain_navigation(&mut self) {
        self.retry_overlay_cleanup();
        self.retry_blocked_cleanup();
        #[cfg(feature = "ui-provider-fixture")]
        self.try_finalize_provider_fixture_removal();
        if intent_bridge::take_overflowed() {
            console::println!("UI_NAV state=rejected reason=callback_queue_full");
        }
        while let Some(action) = intent_bridge::take_intent() {
            match action {
                OwnedShellIntent::Navigate(intent) => {
                    if let Err(error) = self.shell.queue_intent(intent) {
                        console::println!(
                            "UI_NAV state=rejected reason=shell_queue error={:?}",
                            error
                        );
                    }
                    self.drain_shell_navigation();
                }
                OwnedShellIntent::Compose(intent) => self.drain_composition_intent(intent),
                OwnedShellIntent::Refresh(intent) => match intent.intent {
                    RefreshIntent::FullRepaint => intent_bridge::mark_full_repaint_requested(),
                },
                OwnedShellIntent::Configure(intent) => self.drain_settings_intent(intent),
            }
        }
        self.drain_shell_navigation();
    }

    fn drain_settings_intent(&mut self, owned: OwnedUiSettingsIntent) {
        if self
            .active
            .as_ref()
            .is_none_or(|active| active.token != owned.source)
            || self.navigation_faulted
            || self.composition_faulted
            || self.cleanup_blocked.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
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
                        .entry_is_ready_for(id, CatalogueViewKind::OverlaySettings)
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
            .shell
            .queue_intent(OwnedNavIntent {
                source: self.shell.active_instance(),
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
            .overlays
            .iter()
            .find(|overlay| overlay.instance().token.surface == surface)
            .map(|overlay| overlay.instance().token)
        else {
            return true;
        };
        let prepared = match self.shell.prepare_overlay_removal(token) {
            Ok(prepared) => prepared,
            Err(_) => return false,
        };
        let CompositionPlanResult::Removal(dismissal) = prepared.result() else {
            return false;
        };
        self.remove_live_overlay(prepared, dismissal);
        !self
            .overlays
            .iter()
            .any(|overlay| overlay.instance().token.surface == surface)
            && self.overlay_cleanup_blocked.is_empty()
            && !self.composition_faulted
    }

    pub(super) fn drain_composition_intent(&mut self, owned: OwnedCompositionIntent) {
        if self.navigation_faulted
            || self.composition_faulted
            || self.cleanup_blocked.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
        {
            console::println!(
                "UI_COMPOSITION state=rejected reason=cleanup_blocked source={:?}",
                owned.source,
            );
            return;
        }
        let prepared = match self.shell.prepare_composition_intent(owned) {
            Ok(prepared) => prepared,
            Err(error) => {
                console::println!(
                    "UI_COMPOSITION state=rejected reason=prepare source={:?} intent={:?} error={:?}",
                    owned.source,
                    owned.intent,
                    error,
                );
                return;
            }
        };
        match prepared.result() {
            CompositionPlanResult::Admission(OverlayAdmission::Queued(instance)) => {
                match self.shell.commit_overlay_request(prepared) {
                    Ok(OverlayAdmission::Queued(committed)) if committed == instance => {
                        console::println!(
                            "UI_COMPOSITION state=queued token={:?}",
                            committed.token,
                        );
                    }
                    Ok(_) | Err(_) => {
                        console::println!(
                            "UI_COMPOSITION state=rejected reason=queued_commit token={:?}",
                            instance.token,
                        );
                    }
                }
            }
            CompositionPlanResult::Admission(OverlayAdmission::Active(instance)) => {
                self.enter_overlay(prepared, instance);
            }
            CompositionPlanResult::Removal(dismissal) if !dismissal.removed_was_live => {
                match self.shell.commit_overlay_removal(prepared) {
                    Ok(CompositionPlanResult::Removal(committed)) if committed == dismissal => {
                        console::println!(
                            "UI_COMPOSITION state=removed_queued token={:?}",
                            dismissal.removed.token,
                        );
                    }
                    Ok(_) | Err(_) => {
                        console::println!(
                            "UI_COMPOSITION state=rejected reason=queued_remove_commit token={:?}",
                            dismissal.removed.token,
                        );
                    }
                }
            }
            CompositionPlanResult::Removal(dismissal) => {
                self.remove_live_overlay(prepared, dismissal);
            }
            CompositionPlanResult::Cleanup(_) => {
                console::println!("UI_COMPOSITION state=rejected reason=unexpected_cleanup_intent");
            }
        }
    }

    fn enter_overlay(
        &mut self,
        prepared: PreparedComposition<LIVE_OVERLAY_CAPACITY, MODAL_QUEUE_CAPACITY>,
        instance: OverlayInstance,
    ) {
        let mut departures = Vec::<SurfaceInstanceToken, LIVE_OVERLAY_CAPACITY>::new();
        for departing in prepared.delta().leave_live() {
            departures
                .push(departing.token)
                .expect("an admission delta is bounded by live overlay capacity");
        }
        if self.overlays.len().saturating_sub(departures.len()) == LIVE_OVERLAY_CAPACITY {
            console::println!("UI_COMPOSITION state=rejected reason=runtime_capacity");
            return;
        }
        let mut candidates = match self.stage_overlay_entries(prepared.delta().enter_live()) {
            Some(candidates) => candidates,
            None => {
                console::println!(
                    "UI_COMPOSITION state=rejected reason=entry token={:?}",
                    instance.token,
                );
                return;
            }
        };
        if !self.stage_overlay_departures(&departures) {
            self.destroy_uncommitted_overlays(&mut candidates);
            self.navigation_faulted = true;
            self.composition_faulted = true;
            console::println!(
                "UI_COMPOSITION state=fault reason=preemption_runtime_misaligned token={:?}",
                instance.token,
            );
            return;
        }
        match self.shell.commit_overlay_request(prepared) {
            Ok(OverlayAdmission::Active(committed)) if committed == instance => {
                self.complete_overlay_departures(&departures);
                self.activate_overlay_entries(&mut candidates);
                console::println!(
                    "UI_COMPOSITION state=active token={:?} input={:?} preempted={}",
                    instance.token,
                    instance.input,
                    departures.len(),
                );
            }
            Ok(_) | Err(_) => {
                self.restore_overlay_departures(&departures);
                self.destroy_uncommitted_overlays(&mut candidates);
                console::println!(
                    "UI_COMPOSITION state=rejected reason=commit token={:?}",
                    instance.token,
                );
            }
        }
    }

    fn remove_live_overlay(
        &mut self,
        prepared: PreparedComposition<LIVE_OVERLAY_CAPACITY, MODAL_QUEUE_CAPACITY>,
        dismissal: OverlayDismissal,
    ) {
        let mut promotion = match dismissal.promoted {
            Some(instance) => match self.create_overlay_candidate(instance) {
                Ok(candidate) => Some(candidate),
                Err(error) => {
                    console::println!(
                        "UI_COMPOSITION state=rejected reason=promotion_entry token={:?} error={:?}",
                        instance.token,
                        error,
                    );
                    return;
                }
            },
            None => None,
        };
        if promotion
            .as_ref()
            .is_some_and(|candidate| candidate.enable().is_err())
        {
            self.navigation_faulted = true;
            self.composition_faulted = true;
            self.destroy_uncommitted_overlay(
                promotion
                    .take()
                    .expect("the failed promotion remains owned"),
            );
            return;
        }
        let Some(index) = self
            .overlays
            .iter()
            .position(|overlay| overlay.token() == dismissal.removed.token)
        else {
            if let Some(candidate) = promotion {
                self.destroy_uncommitted_overlay(candidate);
            }
            self.navigation_faulted = true;
            self.composition_faulted = true;
            console::println!(
                "UI_COMPOSITION state=fault reason=missing_runtime token={:?}",
                dismissal.removed.token,
            );
            return;
        };
        if self.overlays[index].disable().is_err() {
            self.navigation_faulted = true;
            self.composition_faulted = true;
            if let Some(candidate) = promotion {
                self.destroy_uncommitted_overlay(candidate);
            }
            console::println!(
                "UI_COMPOSITION state=rejected reason=quiesce token={:?}",
                dismissal.removed.token,
            );
            return;
        }
        self.overlays[index].hide();
        match self.shell.commit_overlay_removal(prepared) {
            Ok(CompositionPlanResult::Removal(committed)) if committed == dismissal => {}
            Ok(_) | Err(_) => {
                self.overlays[index].show();
                if self.overlays[index].enable().is_err() {
                    self.navigation_faulted = true;
                    self.composition_faulted = true;
                }
                if let Some(candidate) = promotion {
                    self.destroy_uncommitted_overlay(candidate);
                }
                console::println!(
                    "UI_COMPOSITION state=rolled_back token={:?}",
                    dismissal.removed.token,
                );
                return;
            }
        }

        let removed = self.overlays.remove(index);
        if let Some(candidate) = promotion {
            candidate.show();
            if self.overlays.push(candidate).is_err() {
                panic!("bounded promoted overlay ownership overflowed after live removal");
            }
        }
        let destroy_ok = self.destroy_committed_overlay(removed);
        if dismissal.promoted.is_none() && destroy_ok {
            set_system_layer_capture(false);
        }
        console::println!(
            "UI_COMPOSITION state=dismissed token={:?} promoted={:?} cleanup_blocked={}",
            dismissal.removed.token,
            dismissal.promoted.map(|instance| instance.token),
            !self.overlay_cleanup_blocked.is_empty(),
        );
    }

    fn create_overlay_candidate(
        &self,
        instance: OverlayInstance,
    ) -> Result<ActiveOverlay, OverlayEnterError> {
        if instance.token.surface == self.surfaces.confirm && instance.input == OverlayInput::Modal
        {
            ActiveOverlay::confirm(instance)
        } else if cfg!(feature = "ui-provider-fixture") && {
            #[cfg(feature = "ui-provider-fixture")]
            {
                instance.token.surface == self.surfaces.provider_overlay
                    && instance.input == OverlayInput::Modal
            }
            #[cfg(not(feature = "ui-provider-fixture"))]
            {
                false
            }
        } {
            #[cfg(feature = "ui-provider-fixture")]
            {
                ActiveOverlay::confirm(instance)
            }
            #[cfg(not(feature = "ui-provider-fixture"))]
            {
                Err(OverlayEnterError::ObjectCreation)
            }
        } else {
            Err(OverlayEnterError::ObjectCreation)
        }
    }

    fn destroy_uncommitted_overlay(&mut self, overlay: ActiveOverlay) {
        match overlay.destroy() {
            Ok(()) => {}
            Err(DestroyFailure::Live(overlay)) => {
                if self.overlay_cleanup_blocked.push(overlay).is_err() {
                    panic!("bounded overlay cleanup ownership overflowed");
                }
                self.navigation_faulted = true;
            }
            Err(DestroyFailure::Audit) => {
                self.navigation_faulted = true;
                self.composition_faulted = true;
            }
        }
    }

    fn destroy_committed_overlay(&mut self, overlay: ActiveOverlay) -> bool {
        match overlay.destroy() {
            Ok(()) => true,
            Err(DestroyFailure::Live(overlay)) => {
                if self.overlay_cleanup_blocked.push(overlay).is_err() {
                    panic!("bounded overlay cleanup ownership overflowed");
                }
                self.navigation_faulted = true;
                false
            }
            Err(DestroyFailure::Audit) => {
                self.navigation_faulted = true;
                self.composition_faulted = true;
                false
            }
        }
    }

    pub(super) fn stage_overlay_entries(
        &mut self,
        instances: &[OverlayInstance],
    ) -> Option<Vec<ActiveOverlay, LIVE_OVERLAY_CAPACITY>> {
        let mut candidates = Vec::new();
        for instance in instances {
            let candidate = match self.create_overlay_candidate(*instance) {
                Ok(candidate) => candidate,
                Err(_) => {
                    self.destroy_uncommitted_overlays(&mut candidates);
                    return None;
                }
            };
            if candidate.enable().is_err() {
                self.navigation_faulted = true;
                self.composition_faulted = true;
                self.destroy_uncommitted_overlay(candidate);
                self.destroy_uncommitted_overlays(&mut candidates);
                return None;
            }
            if let Err(candidate) = candidates.push(candidate) {
                self.destroy_uncommitted_overlay(candidate);
                self.destroy_uncommitted_overlays(&mut candidates);
                return None;
            }
        }
        Some(candidates)
    }

    pub(super) fn activate_overlay_entries(
        &mut self,
        candidates: &mut Vec<ActiveOverlay, LIVE_OVERLAY_CAPACITY>,
    ) {
        let exclusive = self.active_surface_owns_screen_exclusively();
        while !candidates.is_empty() {
            let candidate = candidates.remove(0);
            if candidate.is_modal() {
                set_system_layer_capture(true);
            }
            if !exclusive || candidate.is_modal() {
                candidate.show();
            }
            if let Err(candidate) = self.overlays.push(candidate) {
                self.destroy_uncommitted_overlay(candidate);
                self.navigation_faulted = true;
                self.composition_faulted = true;
                panic!("committed overlay entries exceeded the bounded runtime capacity");
            }
        }
    }

    pub(super) fn destroy_uncommitted_overlays(
        &mut self,
        candidates: &mut Vec<ActiveOverlay, LIVE_OVERLAY_CAPACITY>,
    ) {
        while let Some(candidate) = candidates.pop() {
            self.destroy_uncommitted_overlay(candidate);
        }
    }

    pub(super) fn stage_overlay_departures(&mut self, tokens: &[SurfaceInstanceToken]) -> bool {
        for (staged, token) in tokens.iter().enumerate() {
            let Some(index) = self
                .overlays
                .iter()
                .position(|overlay| overlay.token() == *token)
            else {
                self.restore_overlay_departures(&tokens[..staged]);
                return false;
            };
            if self.overlays[index].disable().is_err() {
                self.restore_overlay_departures(&tokens[..staged]);
                self.navigation_faulted = true;
                self.composition_faulted = true;
                return false;
            }
            self.overlays[index].hide();
        }
        true
    }

    pub(super) fn restore_overlay_departures(&mut self, tokens: &[SurfaceInstanceToken]) {
        let exclusive = self.active_surface_owns_screen_exclusively();
        for token in tokens {
            if let Some(overlay) = self
                .overlays
                .iter()
                .find(|overlay| overlay.token() == *token)
            {
                if exclusive && !overlay.is_modal() {
                    overlay.hide();
                } else {
                    overlay.show();
                }
                if overlay.enable().is_err() {
                    self.navigation_faulted = true;
                    self.composition_faulted = true;
                }
            }
        }
    }

    pub(super) fn complete_overlay_departures(&mut self, tokens: &[SurfaceInstanceToken]) {
        for token in tokens {
            let Some(index) = self
                .overlays
                .iter()
                .position(|overlay| overlay.token() == *token)
            else {
                self.navigation_faulted = true;
                self.composition_faulted = true;
                console::println!(
                    "UI_COMPOSITION state=fault reason=missing_runtime token={:?}",
                    token
                );
                continue;
            };
            let overlay = self.overlays.remove(index);
            match overlay.destroy() {
                Ok(()) => {}
                Err(DestroyFailure::Live(overlay)) => {
                    if self.overlay_cleanup_blocked.push(overlay).is_err() {
                        panic!("bounded overlay cleanup ownership overflowed");
                    }
                    self.navigation_faulted = true;
                }
                Err(DestroyFailure::Audit) => {
                    self.navigation_faulted = true;
                    self.composition_faulted = true;
                    console::println!("UI_COMPOSITION state=fault reason=unexpected_overlay_audit");
                }
            }
        }
        if !self.navigation_faulted
            && self.shell.active_modal().is_none()
            && self.overlays.iter().all(|overlay| !overlay.is_modal())
            && self
                .overlay_cleanup_blocked
                .iter()
                .all(|overlay| !overlay.is_modal())
        {
            set_system_layer_capture(false);
        }
    }

    fn retry_overlay_cleanup(&mut self) {
        if self.overlay_cleanup_blocked.is_empty() {
            return;
        }
        let mut blocked = core::mem::replace(&mut self.overlay_cleanup_blocked, Vec::new());
        let mut audit_failed = false;
        while let Some(overlay) = blocked.pop() {
            match overlay.destroy() {
                Ok(()) => {}
                Err(DestroyFailure::Live(overlay)) => {
                    if self.overlay_cleanup_blocked.push(overlay).is_err() {
                        panic!("bounded overlay cleanup ownership overflowed");
                    }
                }
                Err(DestroyFailure::Audit) => {
                    self.navigation_faulted = true;
                    self.composition_faulted = true;
                    audit_failed = true;
                }
            }
        }
        if self.overlay_cleanup_blocked.is_empty()
            && self.cleanup_blocked.is_none()
            && self.active_surface_is_renderable()
            && !audit_failed
            && !self.composition_faulted
            && !self.lifecycle_audit_faulted
        {
            self.navigation_faulted = false;
        }
        if !self.navigation_faulted
            && self.shell.active_modal().is_none()
            && self.overlays.iter().all(|overlay| !overlay.is_modal())
        {
            set_system_layer_capture(false);
        }
    }

    fn retry_blocked_cleanup(&mut self) {
        let Some(blocked) = self.cleanup_blocked.take() else {
            return;
        };
        match blocked.destroy() {
            Ok(()) => {
                if self.composition_faulted || self.lifecycle_audit_faulted {
                    self.navigation_faulted = true;
                } else if self.overlay_cleanup_blocked.is_empty()
                    && self.active_surface_is_renderable()
                {
                    self.navigation_faulted = false;
                } else if let Some(origin) = self.active.take() {
                    self.navigation_faulted = !self.recover_ambient(origin);
                } else {
                    self.navigation_faulted = true;
                }
                console::println!(
                    "UI_NAV state=cleanup_recovered navigation_faulted={}",
                    self.navigation_faulted
                );
            }
            Err(DestroyFailure::Live(blocked)) => {
                self.cleanup_blocked = Some(blocked);
                self.navigation_faulted = true;
            }
            Err(DestroyFailure::Audit) => {
                self.navigation_faulted = true;
                self.lifecycle_audit_faulted = true;
                console::println!("UI_NAV state=fault reason=cleanup_route_audit");
            }
        }
    }
}
