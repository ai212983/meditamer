//! Deadlines owned by the LVGL/product presentation state.
use super::*;

impl Backend {
    pub(crate) fn next_service_ms(&self, now_ms: u64) -> u64 {
        let mut at = self.next_timer_due_ms;
        if let Some(write) = self.settings.next_write_deadline_ms() {
            at = at.min(write);
        }
        for overlay in self.coordinator.live_overlays() {
            if let Some(deadline) = overlay.clock_deadline_ms() {
                at = at.min(deadline);
            }
        }
        if !self.clock_overlay_active() {
            if let Some(screen) = self
                .coordinator
                .active_screen()
                .and_then(|s| match &s.model {
                    SurfaceModel::AmbientView(screen) => Some(screen),
                    _ => None,
                })
            {
                at = at.min(screen.next_deadline_ms());
            }
            if let Some(deadline) = self.analog_clock_deadline(now_ms) {
                at = at.min(deadline);
            }
        }
        if !self.pending_fast_clock.is_none()
            || self.pending_screen_updates.iter().any(Option::is_some)
        {
            at = at.min(now_ms.saturating_add(8));
        }
        at
    }
}
