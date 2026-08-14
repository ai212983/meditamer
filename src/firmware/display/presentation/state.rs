use embassy_time::Instant;

use super::super::panel::{
    lease::{PanelPowerLease, PanelPowerLeasePolicy},
    refresh_tracking::RefreshTracking,
};
use super::touch_equivalence::TouchEquivalenceProbe;
use crate::firmware::ui::lvgl::{Backend, DirtyArea};

pub(in crate::firmware::display) struct PresentationState {
    pub(in crate::firmware::display) backend: Option<Backend>,
    pub(in crate::firmware::display) startup_refresh_complete: bool,
    pub(in crate::firmware::display) last_service_ms: u64,
    pub(in crate::firmware::display) refresh_tracking: RefreshTracking,
    pub(in crate::firmware::display) panel_power_lease: PanelPowerLease,
    pub(in crate::firmware::display) pending_dirty: Option<DirtyArea>,
    pub(in crate::firmware::display) gesture_page_refresh_pending: bool,
    pub(in crate::firmware::display) touch_equivalence: TouchEquivalenceProbe,
}

impl PresentationState {
    pub(in crate::firmware::display) fn new() -> Self {
        Self {
            backend: None,
            startup_refresh_complete: false,
            last_service_ms: Instant::now().as_millis(),
            refresh_tracking: RefreshTracking::new(),
            panel_power_lease: PanelPowerLease::new(PanelPowerLeasePolicy::configured()),
            pending_dirty: None,
            gesture_page_refresh_pending: false,
            touch_equivalence: TouchEquivalenceProbe::new(),
        }
    }

    pub(in crate::firmware::display) fn is_ready(&self) -> bool {
        self.backend.is_some() && self.startup_refresh_complete
    }

    pub(super) fn record_dirty(&mut self, dirty: DirtyArea) {
        self.pending_dirty = Some(
            self.pending_dirty
                .map_or(dirty, |pending| pending.union(dirty)),
        );
    }

    pub(super) fn take_dirty(&mut self) -> Option<DirtyArea> {
        self.pending_dirty.take()
    }
}
