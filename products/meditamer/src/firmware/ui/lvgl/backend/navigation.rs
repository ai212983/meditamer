//! Shell navigation drain.

use super::*;

impl Backend {
    fn begin_input_transition(&mut self) {
        let epoch = crate::firmware::touch::admission::begin_transition();
        self.pending_input_transition = Some(epoch);
        self.input_transition_rendered = false;
        self.input_transition_readiness = InputTransitionReadiness::Ready;
        discard_retired_input();
        crate::firmware::touch::config::TOUCH_LVGL_MULTITOUCH_RESET.store(false, Ordering::Release);
        io::retire_input();
        let _ = self.input.reset(&self.ui_token);
        self.multitouch = LvglMultitouchTracker::default();
        #[cfg(feature = "ui-interaction-trace")]
        crate::firmware::interaction_trace::record(0, 36, 0, epoch, 0);
    }

    pub(crate) fn input_transition_presented(&mut self, clean: bool) {
        if !self.input_transition_rendered
            || self.coordinator.is_faulted()
            || !self.active_surface_is_renderable()
            || !self.input_transition_readiness.allows_presentation(clean)
        {
            return;
        }
        if let Some(epoch) = self.pending_input_transition.take() {
            discard_retired_input();
            crate::firmware::touch::admission::presentation_complete(epoch);
            self.input_transition_rendered = false;
            self.input_transition_readiness = InputTransitionReadiness::Ready;
        }
    }

    pub(super) fn drain_shell_navigation(&mut self) {
        if self.coordinator.is_faulted() {
            while self.coordinator.pop_intent().is_some() {}
            console::println!("UI_NAV state=rejected reason=cleanup_blocked");
            return;
        }

        while let Some(owned) = self.coordinator.pop_intent() {
            #[cfg(feature = "ui-provider-fixture")]
            if matches!(owned.intent, NavIntent::Home)
                && matches!(
                    self.provider_fixture_state,
                    ProviderFixtureState::Registered(owner)
                        if owned.source.surface.owner == owner
                )
            {
                self.execute_provider_removal_fixture();
                self.sync_overlay_visibility_for_active_surface();
                self.sync_exclusive_capture();
                if self.coordinator.navigation_faulted() {
                    break;
                }
                continue;
            }
            let source_is_active_modal = self
                .coordinator
                .shell()
                .active_modal()
                .is_some_and(|modal| modal.token == owned.source);
            let source_authorized = owned.source == self.coordinator.shell().active_instance()
                || source_is_active_modal;
            if !source_authorized {
                console::println!(
                    "UI_NAV state=rejected reason=stale_or_unauthorized source={:?} intent={:?}",
                    owned.source,
                    owned.intent,
                );
                continue;
            }
            if self
                .coordinator
                .shell()
                .active_modal()
                .is_some_and(|modal| modal.token != owned.source)
            {
                console::println!(
                    "UI_NAV state=rejected reason=modal_active source={:?} intent={:?}",
                    owned.source,
                    owned.intent,
                );
                continue;
            }
            let transition_started_us = Instant::now().as_micros();
            self.begin_input_transition();
            let origin_surface = self.coordinator.shell().active().surface;
            // Overlay runtime first: it only borrows `self` for the call,
            // while the screen runtime below holds `&mut analog_clock_shared`
            // and `&mut ambient_shared` across the dispatch.
            let mut overlay_runtime = self.overlay_runtime();
            let mut screen_runtime = lvgl_surface_runtime(
                self.surfaces,
                &self.apps,
                &self.catalogue,
                self.settings.current(),
                self.ui_token.clone(),
                transition_started_us,
                &mut self.analog_clock_shared,
                &mut self.ambient_shared,
            );
            let outcome = self.coordinator.dispatch_navigation(
                owned.intent,
                &mut screen_runtime,
                &mut overlay_runtime,
            );
            self.sync_overlay_visibility_for_active_surface();
            self.sync_exclusive_capture();
            let transition_us = Instant::now()
                .as_micros()
                .saturating_sub(transition_started_us);
            match outcome {
                Err(error) => {
                    console::println!(
                        "UI_NAV state=rejected reason=prepare source={:?} intent={:?} error={:?}",
                        owned.source,
                        owned.intent,
                        error,
                    );
                    continue;
                }
                Ok(NavigationDispatchOutcome::OverlayEntryRejected) => {
                    console::println!(
                        "UI_NAV state=rejected reason=overlay_promotion_entry intent={:?}",
                        owned.intent,
                    );
                    continue;
                }
                Ok(NavigationDispatchOutcome::OverlayRuntimeMisaligned) => {
                    console::println!(
                        "UI_NAV state=fault reason=overlay_runtime_misaligned intent={:?}",
                        owned.intent,
                    );
                }
                Ok(NavigationDispatchOutcome::Committed { changed }) => {
                    if changed {
                        // A callback from the departing screen must not open a
                        // clock on the newly active Ambient Home instance.
                        let _ = intent_bridge::take_ambient_tap_requested();
                        self.pending_fast_clock = PendingFastClock::None;
                        self.input_transition_readiness =
                            InputTransitionReadiness::after_navigation(
                                true,
                                self.coordinator.active_screen().is_some_and(|active| {
                                    matches!(
                                        active.model,
                                        SurfaceModel::AmbientView(_) | SurfaceModel::AnalogClock(_)
                                    )
                                }),
                            );
                        // An overlay-to-Boundary handoff (Clock to Launcher)
                        // is one logical transition presented by a single
                        // Clean scan. Keep ordinary screen-originated
                        // navigation on its existing refresh policy.
                        if source_is_active_modal
                            && self.coordinator.merged_refresh_hint() == RefreshHint::Boundary
                            && self.input_transition_readiness == InputTransitionReadiness::Ready
                        {
                            let active_instance = self.coordinator.shell().active_instance();
                            let clean_slot = Self::screen_update_slot(ScreenUpdateIntent::Clean);
                            self.pending_screen_updates[clean_slot] =
                                Some(OwnedScreenUpdateRequest {
                                    source: active_instance,
                                    request: ScreenUpdateRequest {
                                        intent: ScreenUpdateIntent::Clean,
                                    },
                                });
                            self.input_transition_readiness =
                                self.input_transition_readiness.require_clean();
                        }
                    }
                    console::println!(
                        "UI_NAV state=committed from={:?} to={:?} role={:?} outcome_changed={} transition_us={} cleanup_blocked={}",
                        origin_surface,
                        self.coordinator.shell().active().surface,
                        self.coordinator.shell().active().role,
                        changed,
                        transition_us,
                        self.coordinator.overlay_cleanup_blocked_len() != 0,
                    );
                    self.log_lifecycle_checkpoint("settled_after_delete", transition_us);
                }
                Ok(NavigationDispatchOutcome::RolledBack) => {
                    console::println!(
                        "UI_NAV state=rolled_back from={:?} attempted={:?} transition_us={} cleanup_blocked={} navigation_faulted={}",
                        origin_surface,
                        self.coordinator.shell().active().surface,
                        transition_us,
                        self.coordinator.cleanup_blocked_screen().is_some(),
                        self.coordinator.navigation_faulted(),
                    );
                    self.log_lifecycle_checkpoint("rolled_back", transition_us);
                }
                Ok(NavigationDispatchOutcome::FaultedAfterCommit) => {
                    console::println!(
                        "UI_NAV state=faulted_after_commit from={:?} to={:?} transition_us={}",
                        origin_surface,
                        self.coordinator.shell().active().surface,
                        transition_us,
                    );
                    self.log_lifecycle_checkpoint("cleanup_blocked", transition_us);
                }
            }
            if self.coordinator.navigation_faulted() {
                break;
            }
        }
    }
}

// Admission is closed at both call sites. Free stale queue capacity before
// reopening; epochs still reject a racing publication or merge lookahead.
fn discard_retired_input() {
    use crate::firmware::touch::config::{TOUCH_LVGL_MULTITOUCH_FRAMES, TOUCH_PIPELINE_EVENTS};
    while let Ok(_event) = TOUCH_PIPELINE_EVENTS.try_receive() {
        #[cfg(feature = "ui-interaction-trace")]
        crate::firmware::interaction_trace::record(
            _event.trace_id,
            38,
            _event.time_ms(),
            _event.admission_epoch,
            0,
        );
    }
    while let Ok(_frame) = TOUCH_LVGL_MULTITOUCH_FRAMES.try_receive() {
        #[cfg(feature = "ui-interaction-trace")]
        crate::firmware::interaction_trace::record(
            _frame.trace_id,
            38,
            _frame.t_ms,
            _frame.admission_epoch,
            1,
        );
    }
}
