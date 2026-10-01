//! Interaction correlation adapter over the generic firmware trace collector.

use core::cell::RefCell;
use embassy_sync::blocking_mutex::{raw::CriticalSectionRawMutex, Mutex};

#[derive(Clone, Copy, Default)]
struct Correlation {
    next_id: u32,
    contact: bool,
    physical_id: u32,
    ui_id: u32,
}

static CORRELATION: Mutex<CriticalSectionRawMutex, RefCell<Correlation>> =
    Mutex::new(RefCell::new(Correlation {
        next_id: 0,
        contact: false,
        physical_id: 0,
        ui_id: 0,
    }));

pub(crate) fn install_render_hook() {
    render::interaction_trace::install(hook);
}

#[inline]
pub fn record(flow_id: u32, stage: u8, source_ms: u64, a: u32, b: u32) {
    if !crate::firmware::trace::is_capturing() {
        return;
    }
    tracing::event!(
        name: "stage",
        target: "interaction",
        parent: None,
        tracing::Level::INFO,
        flow_id = u64::from(flow_id),
        stage = u64::from(stage),
        source_ms,
        a = u64::from(a),
        b = u64::from(b),
    );
}

pub fn sample(source_ms: u64, count: u8, x: u16, y: u16) -> u32 {
    let flow_id = CORRELATION.lock(|state| {
        let mut state = state.borrow_mut();
        if count == 0 && !state.contact {
            return 0;
        }
        if count > 0 && !state.contact {
            state.physical_id = match state.next_id.checked_add(1) {
                Some(id) => {
                    state.next_id = id;
                    id
                }
                None => 0,
            };
        }
        let flow_id = state.physical_id;
        state.contact = count > 0;
        if !state.contact {
            state.physical_id = 0;
        }
        flow_id
    });
    if flow_id != 0 && crate::firmware::trace::is_capturing() {
        record(
            flow_id,
            1,
            source_ms,
            (u32::from(x) << 16) | u32::from(y),
            u32::from(count),
        );
    }
    flow_id
}

pub fn ui(id: u32) {
    CORRELATION.lock(|state| state.borrow_mut().ui_id = id);
}

pub fn ui_id() -> u32 {
    if !crate::firmware::trace::is_capturing() {
        return 0;
    }
    CORRELATION.lock(|state| state.borrow().ui_id)
}

fn hook(stage: u8, target: u32, detail: u32) {
    if !crate::firmware::trace::is_capturing() {
        return;
    }
    let flow_id = CORRELATION.lock(|state| state.borrow().ui_id);
    record(flow_id, stage, 0, target, detail);
}

pub fn kind(kind: super::touch::types::TouchEventKind) -> u32 {
    use super::touch::types::TouchEventKind::*;
    match kind {
        Down => 1,
        Move => 2,
        Up => 3,
        Tap => 4,
        LongPress => 5,
        Swipe(_) => 6,
        Cancel => 7,
    }
}

pub struct UiScope;
impl UiScope {
    pub fn enter(id: u32) -> Self {
        ui(id);
        Self
    }
}
impl Drop for UiScope {
    fn drop(&mut self) {
        ui(0);
    }
}
