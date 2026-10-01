//! Correlation state owned by the touch pipeline when interaction tracing is enabled.
//!
//! Contact IDs originate at acquisition. This state deliberately only assigns a
//! per-frame sequence and retains the accepted gesture ID through delayed
//! release and cancellation; neither depends on a later raw sample.

use core::sync::atomic::{AtomicU32, Ordering};

use super::types::TouchEventKind;

static NEXT_FRAME_ID: AtomicU32 = AtomicU32::new(0);

pub(crate) fn next_frame_id() -> u32 {
    next_nonzero(&NEXT_FRAME_ID)
}

fn next_nonzero(counter: &AtomicU32) -> u32 {
    loop {
        let previous = counter.fetch_add(1, Ordering::Relaxed);
        let next = previous.wrapping_add(1);
        if next != 0 {
            return next;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FrameCorrelation {
    pub(crate) contact_id: u32,
    pub(crate) frame_id: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ResetBoundary {
    pub(crate) id: u32,
    pub(crate) generation: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GestureCorrelation {
    active_gesture_id: u32,
    // The HSM can emit Down from a timer-driven continuity sample after the
    // raw frame that started debounce has been released or replaced.
    pending_down_id: u32,
    last_contact_id: u32,
    reset_generation: u32,
}

impl GestureCorrelation {
    pub(crate) fn observe_frame(&mut self, frame: FrameCorrelation) {
        if frame.contact_id != 0 {
            self.last_contact_id = frame.contact_id;
            if self.active_gesture_id == 0 && self.pending_down_id == 0 {
                self.pending_down_id = frame.contact_id;
            }
        }
    }

    /// Call after each engine tick. `pending_down_id` belongs to one HSM
    /// debounce attempt, not to raw contact lifetime; an idle HSM means that
    /// attempt was either completed or aborted.
    pub(crate) fn reconcile_engine(&mut self, engine_idle: bool) {
        if engine_idle {
            self.pending_down_id = 0;
            if self.active_gesture_id == 0 {
                self.last_contact_id = 0;
            }
        }
    }

    pub(crate) fn reset_boundary(&mut self) -> ResetBoundary {
        self.reset_generation = self.reset_generation.wrapping_add(1);
        if self.reset_generation == 0 {
            self.reset_generation = 1;
        }
        ResetBoundary {
            id: self.active_or_last_id(),
            generation: self.reset_generation,
        }
    }

    pub(crate) fn reset_generation(&self) -> u32 {
        self.reset_generation
    }

    pub(crate) fn active_or_last_id(&self) -> u32 {
        if self.active_gesture_id != 0 {
            self.active_gesture_id
        } else if self.pending_down_id != 0 {
            self.pending_down_id
        } else {
            self.last_contact_id
        }
    }

    pub(crate) fn event_id(
        &mut self,
        kind: TouchEventKind,
        frame: Option<FrameCorrelation>,
    ) -> u32 {
        if matches!(kind, TouchEventKind::Down) {
            let id = if self.pending_down_id != 0 {
                self.pending_down_id
            } else {
                frame.map_or(0, |frame| frame.contact_id)
            };
            self.active_gesture_id = id;
            self.pending_down_id = 0;
            return id;
        }
        if self.active_gesture_id != 0 {
            self.active_gesture_id
        } else {
            self.pending_down_id
        }
    }

    /// End after the complete engine output batch so `Up` and its semantic
    /// `Tap`/`Swipe` companion retain the same gesture identity.
    pub(crate) fn finish_batch(&mut self, ended: bool) {
        if ended {
            self.active_gesture_id = 0;
        }
    }
}

pub(crate) const fn ends_gesture(kind: TouchEventKind) -> bool {
    matches!(kind, TouchEventKind::Up | TouchEventKind::Cancel)
}
