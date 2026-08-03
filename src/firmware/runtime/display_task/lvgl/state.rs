use embassy_time::Instant;

use super::{
    panel_power_lease::{PanelPowerLease, PanelPowerLeasePolicy},
    refresh_tracking::RefreshTracking,
};
use crate::firmware::ui::lvgl::{Backend, DirtyArea};

pub(in crate::firmware::runtime::display_task) struct LvglState {
    pub(super) backend: Option<Backend>,
    pub(super) startup_refresh_complete: bool,
    pub(super) last_service_ms: u64,
    pub(super) refresh_tracking: RefreshTracking,
    pub(super) panel_power_lease: PanelPowerLease,
    pub(super) pending_dirty: Option<DirtyArea>,
}

impl LvglState {
    pub(in crate::firmware::runtime::display_task) fn new() -> Self {
        Self {
            backend: None,
            startup_refresh_complete: false,
            last_service_ms: Instant::now().as_millis(),
            refresh_tracking: RefreshTracking::new(),
            panel_power_lease: PanelPowerLease::new(PanelPowerLeasePolicy::configured()),
            pending_dirty: None,
        }
    }

    pub(in crate::firmware::runtime::display_task) fn is_ready(&self) -> bool {
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
