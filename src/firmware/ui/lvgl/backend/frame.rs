//! Per-frame driving: touch intake, timers, and rendering into the panel buffer.

use super::*;

impl Backend {
    pub(crate) fn handle_touch(
        &mut self,
        display: &mut InkplateDriver,
        event: TouchEvent,
    ) -> Option<DirtyArea> {
        io::update_touch(event);
        self.render_with(display, |_| unsafe {
            let input = LVGL_INPUT.load(Ordering::Acquire);
            if !input.is_null() {
                // Force one read per pipeline event so a queued Down+Up pair cannot
                // collapse into a single released sample before LVGL observes it.
                lv::lv_indev_read(input);
            }
        })
    }

    pub(crate) fn handle_multitouch(
        &mut self,
        display: &mut InkplateDriver,
        frame: LvglMultitouchFrame,
    ) -> Option<DirtyArea> {
        let (batch, terminating) = self.multitouch.update_gesture(frame);
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
        if self.shell.active_modal().is_some() {
            console::println!("UI_GESTURE state=blocked reason=modal_active event={event:?}");
            return None;
        }
        self.render_with(display, |backend| {
            if let Some(active) = backend.active.as_mut() {
                let _ = active.show_gesture(event);
            }
        })
    }

    pub(crate) fn run_timers(
        &mut self,
        display: &mut InkplateDriver,
        elapsed_ms: u32,
    ) -> Option<DirtyArea> {
        self.render_with(display, |backend| unsafe {
            let started_us = Instant::now().as_micros();
            backend.timer_metrics.begin_handler(started_us);
            lv::lv_tick_inc(elapsed_ms);
            lv::lv_timer_handler();
            let runtime_us = Instant::now().as_micros().saturating_sub(started_us);
            backend.timer_metrics.finish_handler(runtime_us);
        })
    }

    /// Cheap, monotonic-clock-only check for the active Ambient Home screen:
    /// does anything need a fresh wall-clock read right now? Consumes a
    /// pending background-tap flag either way, so a stale tap left over
    /// from a screen that is no longer active is silently discarded rather
    /// than leaking into a later activation.
    pub(crate) fn ambient_home_poll(&mut self, now_ms: u64) -> ambient_view::AmbientHomeAction {
        let tapped = intent_bridge::take_ambient_tap_requested();
        let Some(screen) = self
            .active
            .as_mut()
            .and_then(ActiveSurface::ambient_view_mut)
        else {
            return ambient_view::AmbientHomeAction::None;
        };
        if tapped {
            screen.handle_tap()
        } else {
            screen.poll(now_ms)
        }
    }

    /// Applies a wall-clock query outcome for `action` to the active
    /// Ambient Home screen, if it is still the active surface. Returns
    /// `true` when the visible surface changed and needs a full-screen
    /// repaint (the caller is responsible for requesting the e-ink hardware
    /// full refresh, matching the plan's "every arc-and-circle render uses
    /// a full-screen update").
    pub(crate) fn ambient_home_apply(
        &mut self,
        action: ambient_view::AmbientHomeAction,
        snapshot: Option<rtc::driver::WallClockSnapshot>,
        now_ms: u64,
    ) -> bool {
        let Some(screen) = self
            .active
            .as_mut()
            .and_then(ActiveSurface::ambient_view_mut)
        else {
            return false;
        };
        unsafe { screen.apply(action, snapshot, now_ms) }
    }

    pub(crate) fn invalidate(&mut self, display: &mut InkplateDriver) -> Option<DirtyArea> {
        self.render_with(display, |_| unsafe {
            let screen = lv::lv_screen_active();
            if !screen.is_null() {
                let _ = lv::lv_obj_invalidate(screen);
            }
        })
    }

    fn read_multitouch(
        &mut self,
        display: &mut InkplateDriver,
        batch: LvglContactBatch,
        cleanup: bool,
    ) -> Option<DirtyArea> {
        self.render_with(display, |_| unsafe {
            let input = LVGL_INPUT.load(Ordering::Acquire);
            if input.is_null() {
                return;
            }
            io::queue_multitouch(batch);
            lv::lv_indev_read(input);
            if cleanup {
                // Advance ENDED/CANCELED recognizers back to NONE before the
                // single-touch path resumes ownership of the pointer indev.
                io::queue_multitouch(LvglContactBatch::default());
                lv::lv_indev_read(input);
            }
        })
    }

    pub(super) fn render_with(
        &mut self,
        display: &mut InkplateDriver,
        update: impl FnOnce(&mut Self),
    ) -> Option<DirtyArea> {
        io::begin(display.framebuffer_bw_mut());
        update(self);
        self.drain_navigation();
        if self.active_surface_is_renderable() {
            unsafe {
                let lv_display = LVGL_DISPLAY.load(Ordering::Acquire);
                if !lv_display.is_null() {
                    lv::lv_refr_now(lv_display);
                }
            }
        }
        io::finish()
    }
}
