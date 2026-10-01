//! Transition-scoped admission for physical and queued touch input.
mod model;
use core::cell::RefCell;
use embassy_sync::blocking_mutex::{raw::CriticalSectionRawMutex, Mutex};
static STATE: Mutex<CriticalSectionRawMutex, RefCell<model::Admission>> =
    Mutex::new(RefCell::new(model::Admission::new()));

pub fn begin_transition() -> u32 {
    STATE.lock(|s| s.borrow_mut().begin_transition())
}
pub fn presentation_complete(epoch: u32) {
    report_completion(STATE.lock(|s| s.borrow_mut().presentation_complete(epoch)));
}
pub fn accepts(epoch: u32) -> bool {
    STATE.lock(|s| s.borrow().accepts(epoch))
}
pub(crate) fn epoch() -> u32 {
    STATE.lock(|s| s.borrow().epoch())
}
pub(crate) fn observe(read_epoch: u32, count: u8) -> bool {
    STATE.lock(|s| s.borrow_mut().observe(read_epoch, count))
}

const _: () = assert!(core::mem::size_of::<model::Admission>() <= 24);

// The request is registered before its wakeup is published. Completing a scan
// cannot admit input until all earlier reset work has actually finished.
pub(crate) fn reset_requested() {
    STATE.lock(|s| s.borrow_mut().reset_requested());
}
pub(crate) fn resets_completed(count: u32) {
    report_completion(STATE.lock(|s| s.borrow_mut().resets_completed(count)));
}
fn report_completion(completed: Option<u32>) {
    if let Some(_epoch) = completed {
        #[cfg(feature = "ui-interaction-trace")]
        crate::firmware::interaction_trace::record(0, 37, 0, _epoch, 0);
        crate::firmware::display::wake();
    }
}
