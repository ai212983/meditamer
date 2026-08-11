const SLOT_COUNT: usize = 2;
const ACTIVE_SLOT_MASK: u8 = 0x03;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LvglTouchPoint {
    pub(crate) x: u16,
    pub(crate) y: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LvglMultitouchFrame {
    pub(crate) t_ms: u64,
    pub(crate) active_mask: u8,
    pub(crate) points: [LvglTouchPoint; SLOT_COUNT],
}

impl LvglMultitouchFrame {
    pub(crate) const fn active_count(self) -> u32 {
        (self.active_mask & ACTIVE_SLOT_MASK).count_ones()
    }

    pub(crate) const fn is_multitouch(self) -> bool {
        self.active_count() > 1
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LvglContactUpdate {
    pub(crate) point: LvglTouchPoint,
    pub(crate) pressed: bool,
    pub(crate) id: u8,
    pub(crate) timestamp: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LvglContactBatch {
    pub(crate) updates: [Option<LvglContactUpdate>; SLOT_COUNT * 2],
}

impl LvglContactBatch {
    pub(crate) fn is_empty(&self) -> bool {
        self.updates.iter().all(Option::is_none)
    }

    fn push(&mut self, update: LvglContactUpdate) {
        if let Some(slot) = self.updates.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(update);
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LvglMultitouchTracker {
    active_mask: u8,
    last_points: [LvglTouchPoint; SLOT_COUNT],
}

impl LvglMultitouchTracker {
    pub(crate) fn update_gesture(
        &mut self,
        frame: LvglMultitouchFrame,
    ) -> (LvglContactBatch, bool) {
        if frame.is_multitouch() {
            return (self.update(frame), false);
        }

        // The pipeline intentionally stops forwarding after the first finger
        // leaves a two-contact gesture. Release every contact LVGL has seen,
        // including the finger that is still physically down, so recognizer
        // motion state cannot leak into the next gesture sequence.
        (self.release_all(frame.t_ms), true)
    }

    pub(crate) fn update(&mut self, frame: LvglMultitouchFrame) -> LvglContactBatch {
        let mut batch = LvglContactBatch::default();
        let active_mask = frame.active_mask & ACTIVE_SLOT_MASK;
        let timestamp = frame.t_ms as u32;

        for slot in 0..SLOT_COUNT {
            let slot_mask = 1 << slot;
            if active_mask & slot_mask != 0 {
                self.last_points[slot] = frame.points[slot];
                batch.push(LvglContactUpdate {
                    point: frame.points[slot],
                    pressed: true,
                    id: slot as u8,
                    timestamp,
                });
            } else if self.active_mask & slot_mask != 0 {
                batch.push(LvglContactUpdate {
                    point: self.last_points[slot],
                    pressed: false,
                    id: slot as u8,
                    timestamp,
                });
            }
        }

        self.active_mask = active_mask;
        batch
    }

    pub(crate) fn release_all(&mut self, t_ms: u64) -> LvglContactBatch {
        self.update(LvglMultitouchFrame {
            t_ms,
            active_mask: 0,
            points: self.last_points,
        })
    }

    pub(crate) fn reset(&mut self) {
        self.active_mask = 0;
    }
}
