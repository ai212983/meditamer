//! Per-frame driving: touch intake, timers, and rendering into the panel buffer.

use super::*;

impl Backend {
    #[cfg(feature = "cpu-load")]
    pub(crate) fn update_cpu_load(&mut self, reading: Option<cpu_load::Snapshot>) {
        self.cpu_reading = reading;
    }

    /// Preserve the latest reading across surfaces. `render_with` applies it
    /// with other ready changes, avoiding an observation-only intermediate scan.
    pub(crate) fn update_environment(&mut self, reading: EnvironmentSnapshot) {
        self.environment_reading = Some(reading);
    }

    pub(crate) fn update_environment_state(&mut self, observation: EnvironmentStateSnapshot) {
        self.environment_state = Some(observation);
    }

    pub(crate) fn handle_touch(
        &mut self,
        display: &mut InkplateDriver,
        event: TouchEvent,
    ) -> Option<DirtyArea> {
        io::update_touch(event);
        self.render_with(display, |backend| {
            // Force one read per pipeline event so a queued Down+Up pair cannot
            // collapse into a single released sample before LVGL observes it.
            let _ = backend.input.read(&backend.ui_token);
        })
    }

    /// Feed one timestamp-ordered single-touch batch through a single LVGL
    /// frame. Every retained event still performs its own `update_touch` plus
    /// `input.read` pair, in order, so each `Down`/`Up`/`Cancel` edge reaches
    /// LVGL as its own sample under the same guarantee as [`handle_touch`];
    /// only the frame publish, navigation drain, and dirty-area scan collapse
    /// into one pass. The caller performs at most one panel refresh for the
    /// whole batch. An empty batch renders nothing.
    pub(crate) fn handle_touch_batch(
        &mut self,
        display: &mut InkplateDriver,
        events: &[TouchEvent],
    ) -> Option<DirtyArea> {
        if events.is_empty() {
            return None;
        }
        self.render_with(display, |backend| {
            for event in events {
                // Each event's callbacks carry that event's own origin; the
                // scope drop resets to zero so no outer scope is needed.
                #[cfg(feature = "ui-interaction-trace")]
                let _trace_scope =
                    crate::firmware::interaction_trace::UiScope::enter(event.trace_id);
                io::update_touch(*event);
                // One read per retained event: a queued Down+Up pair inside
                // the batch cannot collapse into a single released sample
                // before LVGL observes each edge.
                let _ = backend.input.read(&backend.ui_token);
            }
        })
    }

    pub(crate) fn handle_multitouch(
        &mut self,
        display: &mut InkplateDriver,
        frame: LvglMultitouchFrame,
    ) -> Option<DirtyArea> {
        let (batch, terminating) = self.multitouch.update_gesture(frame);
        #[cfg(feature = "ui-interaction-trace")]
        crate::firmware::interaction_trace::record(
            frame.trace_id,
            31,
            frame.t_ms,
            batch.len(),
            u32::from(terminating),
        );
        self.read_multitouch(display, batch, terminating)
    }

    pub(crate) fn reset_multitouch(
        &mut self,
        display: &mut InkplateDriver,
        t_ms: u64,
    ) -> Option<DirtyArea> {
        let releases = self.multitouch.release_all(t_ms);
        if releases.is_empty() {
            self.multitouch.reset();
            return None;
        }
        let rendered = self.read_multitouch(display, releases, true);
        self.multitouch.reset();
        rendered
    }

    pub(crate) fn show_gesture(
        &mut self,
        display: &mut InkplateDriver,
        event: io::LvglGestureEvent,
    ) -> Option<DirtyArea> {
        if self.coordinator.shell().active_modal().is_some() {
            console::println!("UI_GESTURE state=blocked reason=modal_active event={event:?}");
            return None;
        }
        self.render_with(display, |backend| {
            let ui_token = backend.ui_token.clone();
            if let Some(active) = backend.coordinator.active_screen_mut() {
                let _ = active.show_gesture(&ui_token, event);
            }
        })
    }

    pub(crate) fn run_timers(
        &mut self,
        display: &mut InkplateDriver,
        elapsed_ms: u32,
    ) -> Option<DirtyArea> {
        let started_ms = Instant::now().as_millis();
        let mut delay_ms = 1;
        let damage = self.render_with(display, |backend| {
            let started_us = Instant::now().as_micros();
            backend.timer_metrics.begin_handler(started_us);
            delay_ms = backend.ui_token.run_timer_handler(elapsed_ms).unwrap_or(1);
            let runtime_us = Instant::now().as_micros().saturating_sub(started_us);
            backend.timer_metrics.finish_handler(runtime_us);
        });
        self.next_timer_due_ms = started_ms.saturating_add(u64::from(delay_ms.max(1)));
        damage
    }

    /// Cheap, monotonic-clock-only check for the active Ambient Home screen:
    /// does anything need a fresh wall-clock read right now? Consumes a
    /// pending background-tap flag either way, so a stale tap left over
    /// from a screen that is no longer active is silently discarded rather
    /// than leaking into a later activation.
    pub(crate) fn ambient_home_poll(
        &mut self,
        now_ms: u64,
        allow_clean: bool,
    ) -> ambient_view::AmbientHomeAction {
        let tapped = intent_bridge::take_ambient_tap_requested();
        let ambient_active = self
            .coordinator
            .active_screen()
            .is_some_and(|active| matches!(active.model, SurfaceModel::AmbientView(_)));
        if !ambient_active {
            self.pending_fast_clock = PendingFastClock::None;
            return ambient_view::AmbientHomeAction::None;
        }
        let active_instance = self.coordinator.shell().active_instance();
        let clock_instance = self.clock_overlay_token();
        self.pending_fast_clock
            .retain_current(Some(active_instance), clock_instance);
        let clock_active = self.clock_overlay_active();
        if tapped {
            if clock_active {
                return ambient_view::AmbientHomeAction::FetchForShow;
            }
            if self.coordinator.shell().active_modal().is_some() {
                return ambient_view::AmbientHomeAction::None;
            }
            return self
                .coordinator
                .active_screen_mut()
                .and_then(ActiveSurface::ambient_view_mut)
                .map(|screen| screen.handle_tap())
                .unwrap_or(ambient_view::AmbientHomeAction::None);
        }
        if clock_active {
            if allow_clean && self.clock_overlay_timeout_elapsed(now_ms) {
                return ambient_view::AmbientHomeAction::FetchForReturn;
            }
            // A prior Fast render was deferred (board not ready) or its
            // panel push raced (`PartialGateDrainError::NotReady`); the tap
            // that requested it is already consumed, so retry the whole
            // semantic action from here (ADR-0025 item 3) rather than
            // dropping it.
            return if self.pending_fast_clock.is_none() {
                ambient_view::AmbientHomeAction::None
            } else {
                ambient_view::AmbientHomeAction::FetchForShow
            };
        }
        if !self.pending_fast_clock.is_none() {
            return ambient_view::AmbientHomeAction::FetchForShow;
        }
        // Keep the due time intact while upload prevents Clean refreshes.
        // Resume on the first eligible frame rather than repeatedly issuing
        // an action that presentation must reject on every service tick.
        if !allow_clean {
            return ambient_view::AmbientHomeAction::None;
        }
        // The ambient screen borrows mutably out of the coordinator while
        // the shared pack/canvas cache borrows mutably out of `self`:
        // disjoint fields, one poll. The poll drives the pack loader
        // (channel ops only) and reports a pending first composition as an
        // update.
        let (coordinator, ambient_shared) = (&mut self.coordinator, &mut self.ambient_shared);
        coordinator
            .active_screen_mut()
            .and_then(ActiveSurface::ambient_view_mut)
            .map(|screen| screen.poll(ambient_shared, now_ms))
            .unwrap_or(ambient_view::AmbientHomeAction::None)
    }

    /// Applies a wall-clock query outcome to Ambient Home's scheduled arc
    /// movement and, for a deferred timeout, the clock overlay's staged
    /// removal -- both Clean-classified (ADR-0025 item 3), rendered inside
    /// one normal LVGL publish frame so co-ready changes collapse into a
    /// single Clean update. Callers gate this on Clean being allowed before
    /// calling it at all: `AmbientHomeAction::FetchForShow` (Fast clock
    /// admission/update) goes through `render_clock_fast` instead, and is
    /// never passed here.
    pub(crate) fn ambient_home_apply(
        &mut self,
        display: &mut InkplateDriver,
        action: ambient_view::AmbientHomeAction,
        snapshot: Option<rtc::driver::WallClockSnapshot>,
        now_ms: u64,
        request: ScreenUpdateRequest,
    ) -> Option<ScreenUpdate<DirtyArea>> {
        let stage_removal = action == ambient_view::AmbientHomeAction::FetchForReturn;
        if !stage_removal && action != ambient_view::AmbientHomeAction::FetchForUpdate {
            return None;
        }
        let damage = self.render_with(display, |backend| {
            let ui_token = backend.ui_token.clone();
            // Same disjoint borrow as the poll path: the screen out of the
            // coordinator, the pack/canvas cache out of the backend. The
            // composition publishes into the canvas here, inside the LVGL
            // publish frame, so damage tracks the invalidation.
            let applied = if let Some(screen) = backend
                .coordinator
                .active_screen_mut()
                .and_then(ActiveSurface::ambient_view_mut)
            {
                let shared = &mut backend.ambient_shared;
                screen.observe_climate(backend.environment_state, now_ms);
                let _ = screen.apply_snapshot(&ui_token, shared, snapshot, now_ms);
                true
            } else {
                false
            };
            if applied {
                backend.input_transition_readiness = backend
                    .input_transition_readiness
                    .after_ambient_content(true);
            }
            if stage_removal {
                if let Some(token) = backend.clock_overlay_token() {
                    backend.stage_clock_removal(token);
                }
            }
        });
        Some(request.with_damage(damage))
    }

    /// Renders Ambient Home's Fast clock admission/update path (ADR-0025
    /// item 3) inside a normal LVGL publish frame. Callers must preflight
    /// with `context.inkplate.is_partial_refresh_ready()` first -- this
    /// method only renders; pushing the result via the strict partial
    /// waveform is the caller's job (`panel::refresh::
    /// refresh_panel_strict_fast`). Tags the render as unpresented Fast work
    /// so a failed or deferred push can be revalidated and retried.
    pub(crate) fn render_clock_fast(
        &mut self,
        display: &mut InkplateDriver,
        local_epoch_seconds: u32,
        now_ms: u64,
        request: ScreenUpdateRequest,
    ) -> Option<ScreenUpdate<DirtyArea>> {
        let mut applied = false;
        let damage = self.render_with(display, |backend| {
            applied = backend.show_or_refresh_clock_overlay(local_epoch_seconds, now_ms);
        });
        if !applied {
            self.pending_fast_clock = PendingFastClock::None;
            return None;
        }
        if damage.is_some() {
            self.pending_fast_clock = match self.clock_overlay_token() {
                Some(token) => PendingFastClock::AwaitingPush(token),
                None => {
                    PendingFastClock::AwaitingAdmission(self.coordinator.shell().active_instance())
                }
            };
        } else {
            self.pending_fast_clock = PendingFastClock::None;
        }
        Some(request.with_damage(damage))
    }

    /// Promotes a Back-tap clock-removal request into the staged (hidden)
    /// state now that Clean is confirmed allowed, inside a normal LVGL
    /// publish frame (ADR-0025 item 5). A no-op if nothing is requested.
    pub(crate) fn promote_requested_clock_removal(
        &mut self,
        display: &mut InkplateDriver,
        request: ScreenUpdateRequest,
    ) -> Option<ScreenUpdate<DirtyArea>> {
        let current_clock = self.clock_overlay_token();
        self.clock_removal.retain_requested(current_clock);
        let token = self.clock_removal_requested()?;
        let damage = self.render_with(display, |backend| {
            backend.stage_clock_removal(token);
        });
        Some(request.with_damage(damage))
    }

    /// The board was not ready to render Fast clock work at all; retain the
    /// semantic action for a later retry (ADR-0025 item 3).
    pub(crate) fn mark_fast_clock_deferred(&mut self) {
        self.pending_fast_clock = match self.clock_overlay_token() {
            Some(token) => PendingFastClock::AwaitingPush(token),
            None => PendingFastClock::AwaitingAdmission(self.coordinator.shell().active_instance()),
        };
    }

    pub(crate) fn clear_pending_fast_clock(&mut self) {
        self.pending_fast_clock = PendingFastClock::None;
    }

    pub(crate) fn invalidate(&mut self, display: &mut InkplateDriver) -> Option<DirtyArea> {
        self.render_with(display, |backend| {
            let _ = backend.ui_token.invalidate_active_screen();
        })
    }

    fn read_multitouch(
        &mut self,
        display: &mut InkplateDriver,
        batch: LvglContactBatch,
        cleanup: bool,
    ) -> Option<DirtyArea> {
        self.render_with(display, |backend| {
            io::queue_multitouch(batch);
            let _ = backend.input.read(&backend.ui_token);
            if cleanup {
                // Advance ENDED/CANCELED recognizers back to NONE before the
                // single-touch path resumes ownership of the pointer indev.
                io::queue_multitouch(LvglContactBatch::default());
                let _ = backend.input.read(&backend.ui_token);
            }
        })
    }

    pub(super) fn render_with(
        &mut self,
        display: &mut InkplateDriver,
        update: impl FnOnce(&mut Self),
    ) -> Option<DirtyArea> {
        // External mutations may create earlier LVGL timers. Re-evaluate before sleeping.
        self.next_timer_due_ms = 0;
        let ui_token = self.ui_token.clone();
        let frame = ui_token
            .publish_l8_frame(display.framebuffer_bw_mut(), io::blit_flush)
            .ok()?;
        io::begin();
        let started_us = Instant::now().as_micros();
        update(self);
        let updated_us = Instant::now().as_micros();
        // Both full-screen surfaces use the same 600x600 L8 storage. The
        // departing surface is retired by navigation before the destination
        // is presented, so keep one boot-lifetime canvas allocation.
        match (self.ambient_shared.canvas, self.analog_clock_shared.canvas) {
            (Some(canvas), None) => self.analog_clock_shared.canvas = Some(canvas),
            (None, Some(canvas)) => self.ambient_shared.canvas = Some(canvas),
            _ => {}
        }
        self.drain_navigation();
        // The old screen has been destroyed. Map views never escape a render
        // step, so its large pack and base caches can now return to PSRAM.
        let active = self.coordinator.active_screen();
        let ambient_active =
            active.is_some_and(|screen| matches!(screen.model, SurfaceModel::AmbientView(_)));
        let clock_active =
            active.is_some_and(|screen| matches!(screen.model, SurfaceModel::AnalogClock(_)));
        if !ambient_active {
            self.ambient_shared.release_inactive_assets();
        }
        if !clock_active {
            self.analog_clock_shared.release_inactive_resources();
        }
        let navigated_us = Instant::now().as_micros();
        if let (Some(reading), Some(screen)) = (
            self.environment_reading,
            self.coordinator
                .active_screen_mut()
                .and_then(ActiveSurface::ambient_view_mut),
        ) {
            let _ = screen.apply_environment_reading(&self.ui_token, reading);
        }
        let prepared_us = Instant::now().as_micros();
        if self.active_surface_is_renderable() {
            // Even a rollback or visually identical destination needs an
            // explicit presentation boundary before admitting fresh input.
            // Gate on the rendered flag: forcing the full-canvas
            // invalidation on every cycle while the handoff waits for real
            // content re-renders the whole canvas each cycle and starves
            // the CPU0 DMA reads behind it.
            if needs_transition_boundary(
                self.pending_input_transition.is_some(),
                self.input_transition_rendered,
            ) {
                self.input_transition_rendered = false;
                let _ = self.ui_token.invalidate_active_screen();
            }
            #[cfg(feature = "cpu-load")]
            if let Some(screen) = self
                .coordinator
                .active_screen_mut()
                .and_then(ActiveSurface::ambient_view_mut)
            {
                screen.update_cpu_footer(
                    &self.ui_token,
                    Instant::now().as_millis(),
                    self.cpu_reading,
                );
            }
            let refreshed = self.ui_token.refresh_default_display().unwrap_or(false);
            if self.pending_input_transition.is_some() {
                // Keep an already-rendered boundary across noop refreshes;
                // only a pass that actually refreshed (re)marks it.
                self.input_transition_rendered =
                    next_transition_rendered(true, self.input_transition_rendered, refreshed);
            }
        }
        frame.finish();
        let finished_us = Instant::now().as_micros();
        let dirty = io::finish();
        #[cfg(feature = "ui-interaction-trace")]
        if crate::firmware::interaction_trace::ui_id() == 0 {
            // Timer, service, and serial-cycle renders have no proven input
            // predecessor. Keep them explicit instead of borrowing a prior
            // contact's identity.
            crate::firmware::interaction_trace::record(
                0,
                34,
                0,
                u32::from(dirty.is_some()),
                finished_us
                    .saturating_sub(started_us)
                    .min(u64::from(u32::MAX)) as u32,
            );
        }
        if finished_us.saturating_sub(started_us) >= 100_000 {
            console::println!(
                "LVGL_RENDER_SLOW update_us={} navigation_us={} prepare_us={} render_us={} total_us={}",
                updated_us.saturating_sub(started_us),
                navigated_us.saturating_sub(updated_us),
                prepared_us.saturating_sub(navigated_us),
                finished_us.saturating_sub(prepared_us),
                finished_us.saturating_sub(started_us),
            );
        }
        dirty
    }
}

impl Backend {
    /// Whether the runtime analog-clock surface is the committed,
    /// renderable active screen. Staged candidates and failed transitions
    /// never acquire render ownership, and a stale instance never publishes.
    pub(crate) fn analog_clock_active(&self) -> bool {
        self.coordinator
            .active_screen()
            .is_some_and(|active| matches!(active.model, SurfaceModel::AnalogClock(_)))
            && self.active_surface_is_renderable()
    }

    fn analog_clock_screen_mut(
        &mut self,
    ) -> Option<(
        &mut analog_clock::AnalogClockScreen,
        &mut analog_clock::SharedCache,
    )> {
        let active_instance = self.coordinator.shell().active_instance();
        let screen = self
            .coordinator
            .active_screen_mut()
            .and_then(ActiveSurface::analog_clock_mut)?;
        if screen.instance() != active_instance {
            return None;
        }
        Some((screen, &mut self.analog_clock_shared))
    }

    /// One-shot activation probe for the runtime clock surface. Reports the
    /// full first-poll gate vector once per screen instance; deliberately
    /// NOT gated on the instance match so a stale-instance wedge reports
    /// itself instead of staying silent.
    pub(crate) fn analog_clock_report_activation(&mut self, now_ms: u64) {
        let active_instance = self.coordinator.shell().active_instance();
        let ui_token = self.ui_token.clone();
        let (is_clock, frame_match, token_match) = self
            .coordinator
            .active_screen()
            .map(|active| {
                (
                    matches!(active.model, SurfaceModel::AnalogClock(_)),
                    active.frame == self.coordinator.shell().active(),
                    active.token == active_instance,
                )
            })
            .unwrap_or((false, false, false));
        if !is_clock {
            return;
        }
        let Some(screen) = self
            .coordinator
            .active_screen_mut()
            .and_then(ActiveSurface::analog_clock_mut)
        else {
            return;
        };
        let lvgl = screen.root_widget().is_active_screen(&ui_token);
        screen.report_activation_once(active_instance, lvgl, frame_match, token_match, now_ms);
    }

    /// Monotonic-clock-only check for the active clock screen. Takes `&mut`
    /// because an idle anchored screen installs its next render target
    /// (prefetched upcoming minute or catch-up current minute) here, with
    /// no RTC traffic. Never touches RTC/I2C itself.
    pub(crate) fn analog_clock_poll(&mut self, now_ms: u64) -> analog_clock::ClockPoll {
        if !self.analog_clock_active() {
            return analog_clock::ClockPoll::Inactive;
        }
        let active_instance = self.coordinator.shell().active_instance();
        self.coordinator
            .active_screen_mut()
            .and_then(|active| match &mut active.model {
                SurfaceModel::AnalogClock(screen) => Some(screen.poll(now_ms, active_instance)),
                _ => None,
            })
            .unwrap_or(analog_clock::ClockPoll::Inactive)
    }

    /// Anchor-estimated current epoch minute, if the screen is anchored.
    /// Lets the presentation layer hold future frames with zero RTC cost.
    pub(crate) fn analog_clock_estimated_current(&self, now_ms: u64) -> Option<u64> {
        self.coordinator
            .active_screen()
            .and_then(|active| match &active.model {
                SurfaceModel::AnalogClock(screen) => screen.estimated_current(now_ms),
                _ => None,
            })
    }

    /// Apply a freshly fetched wall-clock snapshot to the clock target.
    pub(crate) fn analog_clock_observe(
        &mut self,
        snapshot: Option<rtc::driver::WallClockSnapshot>,
        now_ms: u64,
    ) {
        if let Some((screen, _)) = self.analog_clock_screen_mut() {
            screen.observe_time(snapshot, now_ms);
        }
    }

    /// Advance loader/base/frame work by the per-tick row budgets. Touches
    /// only PSRAM staging buffers, never the published canvas or the panel,
    /// and never awaits the SD read: the display task keeps owning I2C/SD
    /// power between these calls.
    pub(crate) fn analog_clock_service(&mut self) -> Option<analog_clock::TargetMinute> {
        let (screen, shared) = self.analog_clock_screen_mut()?;
        screen.service(shared)
    }

    /// Freshness-check a completed frame against fresh wall time.
    pub(crate) fn analog_clock_settle(
        &mut self,
        snapshot: Option<rtc::driver::WallClockSnapshot>,
        completed: analog_clock::TargetMinute,
        now_ms: u64,
    ) -> analog_clock::SettleOutcome {
        self.analog_clock_screen_mut()
            .map(|(screen, _)| screen.settle(snapshot, completed, now_ms))
            .unwrap_or(analog_clock::SettleOutcome::Held)
    }

    /// RTC-free freshness gate for a completed Fast frame, using the
    /// screen's anchor estimate. Never spends an RTC round trip: ordinary
    /// minutes publish through here, while Clean minutes keep the
    /// snapshot-validated [`settle`](Self::analog_clock_settle) path so the
    /// boundary read doubles as the anchor resync.
    pub(crate) fn analog_clock_settle_estimated(
        &mut self,
        completed: analog_clock::TargetMinute,
        now_ms: u64,
    ) -> analog_clock::EstimatedSettle {
        self.analog_clock_screen_mut()
            .map(|(screen, _)| screen.settle_estimated(completed, now_ms))
            .unwrap_or(analog_clock::EstimatedSettle::StaleRetarget {
                epoch_minute: completed.epoch_minute,
            })
    }

    /// Record that the panel covered `target`. Only this advances the
    /// clock's minute deduplication; an intended refresh the scheduler
    /// rejects stays pending for retry.
    pub(crate) fn analog_clock_confirm_published(
        &mut self,
        target: analog_clock::TargetMinute,
        now_ms: u64,
    ) {
        if let Some((screen, _)) = self.analog_clock_screen_mut() {
            screen.confirm_published(target, now_ms);
        }
    }

    /// Whether the active clock screen failed (missing assets, PSRAM, or
    /// render) and still needs its failure placeholder painted.
    pub(crate) fn analog_clock_failure_pending(&self) -> bool {
        self.coordinator
            .active_screen()
            .is_some_and(|active| match &active.model {
                SurfaceModel::AnalogClock(screen) => screen.failure_placeholder_needed(),
                _ => false,
            })
    }

    /// Paint the clock failure placeholder once. Best-effort: never touches
    /// staging or the panel.
    pub(crate) fn analog_clock_paint_failure(&mut self, display: &mut InkplateDriver) {
        if !self.analog_clock_failure_pending() {
            return;
        }
        self.render_with(display, |backend| {
            let ui_token = backend.ui_token.clone();
            let active_instance = backend.coordinator.shell().active_instance();
            let canvas = backend.analog_clock_shared.canvas;
            let screen = backend
                .coordinator
                .active_screen_mut()
                .and_then(ActiveSurface::analog_clock_mut);
            match (screen, canvas) {
                (Some(screen), Some(canvas)) if screen.instance() == active_instance => {
                    screen.paint_failure_placeholder(&ui_token, canvas);
                }
                _ => {}
            }
        });
    }

    /// Atomically publish a settled frame into the retained canvas inside
    /// a normal LVGL publish frame. Call only after
    /// [`analog_clock::SettleOutcome::Published`]; never presents unfinished
    /// rows or a buffer under construction. Returns `None` when the
    /// staging-to-canvas copy did not land (missing/stale instance or a
    /// failed canvas write) so callers never confirm a frame the panel
    /// did not cover.
    pub(crate) fn analog_clock_publish(
        &mut self,
        display: &mut InkplateDriver,
    ) -> Option<DirtyArea> {
        if !self.analog_clock_active() {
            return None;
        }
        analog_clock::begin_composition(&self.analog_clock_shared);
        let started = embassy_time::Instant::now().as_micros();
        let mut landed = false;
        let damage = self.render_with(display, |backend| {
            let ui_token = backend.ui_token.clone();
            let active_instance = backend.coordinator.shell().active_instance();
            let canvas = backend.analog_clock_shared.canvas;
            let screen = backend
                .coordinator
                .active_screen_mut()
                .and_then(ActiveSurface::analog_clock_mut);
            match (screen, canvas) {
                (Some(screen), Some(canvas)) if screen.instance() == active_instance => {
                    if screen.publish_staging(&ui_token, canvas) {
                        landed = true;
                        // Real staged content landed in the retained canvas:
                        // advance the navigation handoff guard the same way
                        // the ambient-clean path does. A failed copy (or a
                        // loading/mismatched surface, which never reaches
                        // here) leaves the guard waiting, so Clean-gated
                        // input is never enabled early.
                        backend.input_transition_readiness = backend
                            .input_transition_readiness
                            .after_ambient_content(true);
                    }
                }
                _ => {}
            }
        });
        if landed {
            analog_clock::report_composition(
                &self.analog_clock_shared,
                embassy_time::Instant::now()
                    .as_micros()
                    .saturating_sub(started),
            );
        }
        damage.filter(|_| landed)
    }

    /// Monotonic deadline for the next clock wall-clock read, retry, or
    /// cooperative row budget.
    pub(crate) fn analog_clock_deadline(&self, now_ms: u64) -> Option<u64> {
        self.coordinator
            .active_screen()
            .and_then(|active| match &active.model {
                SurfaceModel::AnalogClock(screen) => Some(screen.next_deadline_ms(now_ms)),
                _ => None,
            })
    }
}
