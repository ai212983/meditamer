mod core;

use meditamer::inkplate_hal::TouchSample as HalTouchSample;

use crate::app::types::{TouchEvent, TouchEventKind, TouchSwipeDirection};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TouchEngineOutput {
    pub(crate) events: [Option<TouchEvent>; 3],
}

pub(crate) struct TouchEngine {
    inner: core::TouchEngine,
    last_primary: Option<core::TouchPoint>,
}

impl Default for TouchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TouchEngine {
    pub(crate) fn new() -> Self {
        Self {
            inner: core::TouchEngine::new(),
            last_primary: None,
        }
    }

    pub(crate) fn tick(&mut self, now_ms: u64, sample: HalTouchSample) -> TouchEngineOutput {
        let primary = self.select_primary_point(sample);
        let normalized_count = if primary.is_some() { 1 } else { 0 };
        let core_sample = core::TouchSample {
            touch_count: normalized_count,
            points: [primary.unwrap_or_default(), core::TouchPoint::default()],
        };

        let output = self.inner.tick(now_ms, core_sample);
        TouchEngineOutput {
            events: output.events.map(|item| item.map(map_event)),
        }
    }

    fn select_primary_point(&mut self, sample: HalTouchSample) -> Option<core::TouchPoint> {
        let mut candidates = [core::TouchPoint::default(); 2];
        let mut candidate_count = 0usize;

        for point in sample.points {
            if point.x == 0 && point.y == 0 {
                continue;
            }
            if candidate_count < candidates.len() {
                candidates[candidate_count] = core::TouchPoint {
                    x: point.x,
                    y: point.y,
                };
                candidate_count += 1;
            }
        }

        if candidate_count == 0 {
            self.last_primary = None;
            return None;
        }

        let selected = if candidate_count == 1 || self.last_primary.is_none() {
            candidates[0]
        } else {
            let prev = self.last_primary.unwrap_or_default();
            let a = candidates[0];
            let b = candidates[1];
            if squared_distance(a, prev) <= squared_distance(b, prev) {
                a
            } else {
                b
            }
        };

        self.last_primary = Some(selected);
        Some(selected)
    }
}

fn squared_distance(a: core::TouchPoint, b: core::TouchPoint) -> u32 {
    let dx = a.x as i32 - b.x as i32;
    let dy = a.y as i32 - b.y as i32;
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy)) as u32
}

fn map_event(event: core::TouchEvent) -> TouchEvent {
    TouchEvent {
        kind: map_kind(event.kind),
        t_ms: event.t_ms,
        x: event.x,
        y: event.y,
        start_x: event.start_x,
        start_y: event.start_y,
        duration_ms: event.duration_ms,
        touch_count: event.touch_count,
    }
}

fn map_kind(kind: core::TouchEventKind) -> TouchEventKind {
    match kind {
        core::TouchEventKind::Down => TouchEventKind::Down,
        core::TouchEventKind::Move => TouchEventKind::Move,
        core::TouchEventKind::Up => TouchEventKind::Up,
        core::TouchEventKind::Tap => TouchEventKind::Tap,
        core::TouchEventKind::LongPress => TouchEventKind::LongPress,
        core::TouchEventKind::Swipe(direction) => {
            TouchEventKind::Swipe(map_swipe_direction(direction))
        }
        core::TouchEventKind::Cancel => TouchEventKind::Cancel,
    }
}

fn map_swipe_direction(direction: core::TouchSwipeDirection) -> TouchSwipeDirection {
    match direction {
        core::TouchSwipeDirection::Left => TouchSwipeDirection::Left,
        core::TouchSwipeDirection::Right => TouchSwipeDirection::Right,
        core::TouchSwipeDirection::Up => TouchSwipeDirection::Up,
        core::TouchSwipeDirection::Down => TouchSwipeDirection::Down,
    }
}
