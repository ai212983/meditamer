#![forbid(unsafe_code)]

use core::cell::RefCell;

use super::{raw, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind};
use embassy_sync::blocking_mutex::{raw::CriticalSectionRawMutex, Mutex};
use heapless::Vec;

/// Upper bound on live widgets awaiting a later LVGL deletion attempt.
pub const ORPHANED_WIDGET_CAPACITY: usize = 4;

/// Pointer identity stored as an address so the static remains `Sync`.
///
/// Only the single UI task may park or restore widgets; this value must never
/// cross an execution context.
#[derive(Clone, Copy)]
struct ParkedWidget {
    addr: usize,
    generation: u32,
    runtime_id: u32,
    kind: WidgetKind,
}

impl ParkedWidget {
    fn from_widget(widget: Widget) -> Self {
        Self {
            addr: widget.addr,
            generation: widget.generation,
            runtime_id: widget.runtime_id,
            kind: widget.kind,
        }
    }

    fn into_widget(self) -> Widget {
        Widget {
            obj: raw::Object::from_registered_addr(self.addr),
            addr: self.addr,
            generation: self.generation,
            runtime_id: self.runtime_id,
            kind: self.kind,
        }
    }
}

static ORPHANED_WIDGETS: Mutex<
    CriticalSectionRawMutex,
    RefCell<Vec<ParkedWidget, ORPHANED_WIDGET_CAPACITY>>,
> = Mutex::new(RefCell::new(Vec::new()));

/// Parks a still-live widget after rollback deletion fails.
///
/// On overflow, ownership returns to the caller; a live widget must never be
/// silently discarded.
pub fn park_orphaned_widget(widget: Widget) -> Result<(), Widget> {
    let parked = ParkedWidget::from_widget(widget);
    let pushed =
        ORPHANED_WIDGETS.lock(|parked_widgets| parked_widgets.borrow_mut().push(parked).is_ok());
    if pushed {
        Ok(())
    } else {
        Err(parked.into_widget())
    }
}

/// Parks a widget for retry, failing fast if the bounded retry queue is full.
/// Callers that cannot retain the returned widget themselves must use this
/// instead of abandoning it while it may still be live.
#[track_caller]
pub fn park_orphaned_widget_or_panic(widget: Widget) {
    assert!(
        park_orphaned_widget(widget).is_ok(),
        "LVGL orphaned-widget retry queue overflow"
    );
}

/// Retries every parked widget and returns the number still live.
pub fn retry_parked_widgets(token: &UiAccessToken) -> usize {
    let pending: Vec<ParkedWidget, ORPHANED_WIDGET_CAPACITY> =
        ORPHANED_WIDGETS.lock(|parked_widgets| core::mem::take(&mut *parked_widgets.borrow_mut()));
    let mut still_parked: Vec<ParkedWidget, ORPHANED_WIDGET_CAPACITY> = Vec::new();
    for parked in pending {
        if let Err(WidgetDeleteFailure::StillValid(widget)) = parked.into_widget().delete(token) {
            // `still_parked` cannot exceed the fixed-capacity input batch.
            let _ = still_parked.push(ParkedWidget::from_widget(widget));
        }
    }
    let remaining = still_parked.len();
    ORPHANED_WIDGETS.lock(|parked_widgets| *parked_widgets.borrow_mut() = still_parked);
    remaining
}

pub(super) fn clear() {
    ORPHANED_WIDGETS.lock(|parked_widgets| parked_widgets.borrow_mut().clear());
}
