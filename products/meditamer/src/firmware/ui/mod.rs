pub(crate) mod apps;
pub mod asset_bundles;
pub(crate) mod lvgl;
mod overlay;
mod screen;
mod widget;

use core::sync::atomic::{AtomicU8, Ordering};

// Correlated service queries must not depend on lossy UART diagnostics. The
// active Ambient screen owns these bits and clears them on release/rearm.
static AMBIENT_MOUNTAIN_STATUS: AtomicU8 = AtomicU8::new(0);

pub(crate) fn ambient_mountain_status() -> (bool, bool) {
    let status = AMBIENT_MOUNTAIN_STATUS.load(Ordering::Acquire);
    (status & 1 != 0, status & 2 != 0)
}

pub(crate) fn clear_ambient_mountain_status() {
    AMBIENT_MOUNTAIN_STATUS.store(0, Ordering::Release);
}

pub(crate) fn mark_ambient_mountain_adopted() {
    AMBIENT_MOUNTAIN_STATUS.store(1, Ordering::Release);
}

pub(crate) fn mark_ambient_mountain_composed() {
    AMBIENT_MOUNTAIN_STATUS.fetch_or(2, Ordering::Release);
}
