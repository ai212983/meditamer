//! Shell navigation drain and ambient-surface recovery.

use super::*;

impl Backend {
    pub(super) fn drain_shell_navigation(&mut self) {
        if self.navigation_faulted
            || self.composition_faulted
            || self.cleanup_blocked.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
        {
            while self.shell.pop_intent().is_some() {}
            esp_println::println!("UI_NAV state=rejected reason=cleanup_blocked");
            return;
        }

        while let Some(owned) = self.shell.pop_intent() {
            #[cfg(feature = "ui-provider-fixture")]
            if matches!(owned.intent, NavIntent::Home)
                && matches!(
                    self.provider_fixture_state,
                    ProviderFixtureState::Registered(owner)
                        if owned.source.surface.owner == owner
                )
            {
                self.execute_provider_removal_fixture();
                if self.navigation_faulted {
                    break;
                }
                continue;
            }
            if self.shell.active_modal().is_some() {
                esp_println::println!(
                    "UI_NAV state=rejected reason=modal_active source={:?} intent={:?}",
                    owned.source,
                    owned.intent,
                );
                continue;
            }
            let transition_started_us = Instant::now().as_micros();
            let prepared = match self.shell.prepare_intent(owned.intent) {
                Ok(prepared) => prepared,
                Err(error) => {
                    esp_println::println!(
                        "UI_NAV state=rejected reason=prepare source={:?} intent={:?} error={:?}",
                        owned.source,
                        owned.intent,
                        error
                    );
                    continue;
                }
            };
            let mut entering_overlays =
                match self.stage_overlay_entries(prepared.composition_delta().enter_live()) {
                    Some(candidates) => candidates,
                    None => {
                        esp_println::println!(
                            "UI_NAV state=rejected reason=overlay_promotion_entry intent={:?}",
                            owned.intent
                        );
                        continue;
                    }
                };
            let mut departing_overlays = Vec::<SurfaceInstanceToken, LIVE_OVERLAY_CAPACITY>::new();
            for instance in prepared.composition_delta().leave_live() {
                departing_overlays
                    .push(instance.token)
                    .expect("the navigation delta is bounded by live overlay capacity");
            }
            if !self.stage_overlay_departures(&departing_overlays) {
                self.destroy_uncommitted_overlays(&mut entering_overlays);
                self.navigation_faulted = true;
                self.composition_faulted = true;
                esp_println::println!(
                    "UI_NAV state=fault reason=overlay_runtime_misaligned intent={:?}",
                    owned.intent
                );
                break;
            }
            let origin = prepared.origin();
            let destination = prepared.destination();
            let active_instance = self.active.as_ref().map(|active| active.token);
            if active_instance.is_some_and(|active| !prepared.requires_reentry(active)) {
                match self.shell.commit_navigation(prepared) {
                    Ok(outcome) => {
                        self.complete_overlay_departures(&departing_overlays);
                        self.activate_overlay_entries(&mut entering_overlays);
                        self.sync_overlay_visibility_for_active_surface();
                        esp_println::println!(
                            "UI_NAV state=committed from={:?} to={:?} role={:?} outcome={:?}",
                            origin.surface,
                            destination.surface,
                            destination.role,
                            outcome
                        );
                    }
                    Err(error) => {
                        self.restore_overlay_departures(&departing_overlays);
                        self.destroy_uncommitted_overlays(&mut entering_overlays);
                        self.sync_overlay_visibility_for_active_surface();
                        esp_println::println!(
                            "UI_NAV state=rolled_back from={:?} attempted={:?} error={:?}",
                            origin.surface,
                            destination.surface,
                            error
                        );
                    }
                }
                continue;
            }

            let destination_instance = prepared.destination_instance();
            let Some(origin_instance) = self.active.take() else {
                self.restore_overlay_departures(&departing_overlays);
                self.destroy_uncommitted_overlays(&mut entering_overlays);
                self.navigation_faulted = true;
                esp_println::println!("UI_NAV state=fault reason=missing_active_instance");
                break;
            };
            let result = {
                let mut runtime = LvglSurfaceRuntime {
                    surfaces: self.surfaces,
                    catalogue: &self.catalogue,
                    settings: self.settings.current(),
                    transition_started_us,
                };
                execute_transition(
                    &mut runtime,
                    origin_instance,
                    destination,
                    destination_instance,
                    || self.shell.commit_navigation(prepared),
                )
            };
            let transition_us = Instant::now()
                .as_micros()
                .saturating_sub(transition_started_us);

            match result {
                TransitionResult::Committed { active, outcome } => {
                    self.active = Some(active);
                    self.complete_overlay_departures(&departing_overlays);
                    self.activate_overlay_entries(&mut entering_overlays);
                    self.sync_overlay_visibility_for_active_surface();
                    esp_println::println!(
                        "UI_NAV state=committed from={:?} to={:?} role={:?} outcome={:?} transition_us={} cleanup_blocked={}",
                        origin.surface,
                        destination.surface,
                        destination.role,
                        outcome,
                        transition_us,
                        !self.overlay_cleanup_blocked.is_empty(),
                    );
                    self.log_lifecycle_checkpoint("settled_after_delete", transition_us);
                }
                TransitionResult::RolledBack {
                    active,
                    cleanup_blocked,
                    cleanup_audit_failed,
                    reason,
                } => {
                    if matches!(reason, RollbackReason::Entry(_)) {
                        let _ = self.catalogue.mark_surface_faulted(destination.surface);
                    }
                    self.lifecycle_audit_faulted |= cleanup_audit_failed;
                    self.restore_overlay_departures(&departing_overlays);
                    self.destroy_uncommitted_overlays(&mut entering_overlays);
                    self.cleanup_blocked = cleanup_blocked;
                    self.settle_rolled_back(active, cleanup_audit_failed, &reason);
                    self.sync_overlay_visibility_for_active_surface();
                    esp_println::println!(
                        "UI_NAV state=rolled_back from={:?} attempted={:?} reason={:?} transition_us={} cleanup_blocked={} navigation_faulted={}",
                        origin.surface,
                        destination.surface,
                        reason,
                        transition_us,
                        self.cleanup_blocked.is_some(),
                        self.navigation_faulted,
                    );
                    self.log_lifecycle_checkpoint("rolled_back", transition_us);
                }
                TransitionResult::FaultedAfterCommit {
                    active,
                    cleanup_blocked,
                    cleanup_audit_failed,
                    outcome,
                    reason,
                } => {
                    self.lifecycle_audit_faulted |= cleanup_audit_failed;
                    self.active = Some(active);
                    self.complete_overlay_departures(&departing_overlays);
                    self.activate_overlay_entries(&mut entering_overlays);
                    self.cleanup_blocked = cleanup_blocked;
                    self.navigation_faulted = true;
                    self.sync_overlay_visibility_for_active_surface();
                    esp_println::println!(
                        "UI_NAV state=faulted_after_commit from={:?} to={:?} outcome={:?} reason={:?} transition_us={} cleanup_audit_failed={}",
                        origin.surface,
                        destination.surface,
                        outcome,
                        reason,
                        transition_us,
                        cleanup_audit_failed,
                    );
                    self.log_lifecycle_checkpoint("cleanup_blocked", transition_us);
                }
            }
            if self.navigation_faulted {
                break;
            }
        }
    }

    /// Restores the active surface after a rolled-back transition and decides
    /// whether the rollback left navigation in a faulted state.
    fn settle_rolled_back<EnterError, CommitError>(
        &mut self,
        active: ActiveSurface,
        cleanup_audit_failed: bool,
        reason: &RollbackReason<EnterError, CommitError>,
    ) {
        let origin_restored = match reason {
            RollbackReason::Entry(_) => true,
            RollbackReason::Activation { origin_restored }
            | RollbackReason::Quiesce { origin_restored }
            | RollbackReason::CandidateEnable { origin_restored }
            | RollbackReason::Commit {
                origin_restored, ..
            } => *origin_restored,
        };

        if origin_restored {
            self.active = Some(active);
            self.navigation_faulted = self.cleanup_blocked.is_some() || cleanup_audit_failed;
        } else if self.cleanup_blocked.is_none() && !cleanup_audit_failed {
            self.navigation_faulted = !self.recover_ambient(active);
        } else {
            self.active = Some(active);
            self.navigation_faulted = true;
        }
    }

    pub(super) fn recover_ambient(&mut self, origin: ActiveSurface) -> bool {
        let transition_started_us = Instant::now().as_micros();
        let prepared = match self.shell.prepare_recovery_home() {
            Ok(prepared) => prepared,
            Err(error) => {
                self.active = Some(origin);
                esp_println::println!(
                    "UI_NAV state=recovery_failed stage=prepare error={:?}",
                    error
                );
                return false;
            }
        };
        let mut entering_overlays = match self
            .stage_overlay_entries(prepared.composition_delta().enter_live())
        {
            Some(candidates) => candidates,
            None => {
                self.active = Some(origin);
                esp_println::println!("UI_NAV state=recovery_failed stage=overlay_promotion_entry");
                return false;
            }
        };
        let mut departing_overlays = Vec::<SurfaceInstanceToken, LIVE_OVERLAY_CAPACITY>::new();
        for instance in prepared.composition_delta().leave_live() {
            departing_overlays
                .push(instance.token)
                .expect("the recovery delta is bounded by live overlay capacity");
        }
        if !self.stage_overlay_departures(&departing_overlays) {
            self.destroy_uncommitted_overlays(&mut entering_overlays);
            self.active = Some(origin);
            esp_println::println!("UI_NAV state=recovery_failed stage=overlay_runtime_misaligned");
            return false;
        }
        let destination = prepared.destination();
        let destination_instance = prepared.destination_instance();
        let mut runtime = LvglSurfaceRuntime {
            surfaces: self.surfaces,
            catalogue: &self.catalogue,
            settings: self.settings.current(),
            transition_started_us,
        };
        match execute_transition(
            &mut runtime,
            origin,
            destination,
            destination_instance,
            || self.shell.commit_navigation(prepared),
        ) {
            TransitionResult::Committed { active, outcome } => {
                self.active = Some(active);
                self.complete_overlay_departures(&departing_overlays);
                self.activate_overlay_entries(&mut entering_overlays);
                self.cleanup_blocked = None;
                self.sync_overlay_visibility_for_active_surface();
                esp_println::println!(
                    "UI_NAV state=recovered destination={:?} outcome={:?}",
                    destination.surface,
                    outcome
                );
                self.active_surface_is_renderable()
            }
            TransitionResult::RolledBack {
                active,
                cleanup_blocked,
                cleanup_audit_failed,
                reason,
            } => {
                self.lifecycle_audit_faulted |= cleanup_audit_failed;
                self.restore_overlay_departures(&departing_overlays);
                self.destroy_uncommitted_overlays(&mut entering_overlays);
                self.active = Some(active);
                self.cleanup_blocked = cleanup_blocked;
                self.sync_overlay_visibility_for_active_surface();
                esp_println::println!(
                    "UI_NAV state=recovery_failed stage=transition reason={:?} cleanup_blocked={} cleanup_audit_failed={}",
                    reason,
                    self.cleanup_blocked.is_some(),
                    cleanup_audit_failed
                );
                false
            }
            TransitionResult::FaultedAfterCommit {
                active,
                cleanup_blocked,
                cleanup_audit_failed,
                outcome,
                reason,
            } => {
                self.lifecycle_audit_faulted |= cleanup_audit_failed;
                self.active = Some(active);
                self.complete_overlay_departures(&departing_overlays);
                self.activate_overlay_entries(&mut entering_overlays);
                self.cleanup_blocked = cleanup_blocked;
                self.sync_overlay_visibility_for_active_surface();
                esp_println::println!(
                    "UI_NAV state=recovery_cleanup_blocked outcome={:?} reason={:?} cleanup_audit_failed={}",
                    outcome,
                    reason,
                    cleanup_audit_failed
                );
                false
            }
        }
    }
}
