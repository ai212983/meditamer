mod core;

use meditamer::inkplate_hal::TouchSample as HalTouchSample;

use crate::app::types::{TouchEvent, TouchEventKind, TouchSwipeDirection};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TouchEngineOutput {
    pub(crate) events: [Option<TouchEvent>; 3],
}

pub(crate) struct TouchEngine {
    inner: core::TouchEngine,
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
        }
    }

    pub(crate) fn tick(&mut self, now_ms: u64, sample: HalTouchSample) -> TouchEngineOutput {
        let core_sample = core::TouchSample {
            touch_count: sample.touch_count,
            points: [
                core::TouchPoint {
                    x: sample.points[0].x,
                    y: sample.points[0].y,
                },
                core::TouchPoint {
                    x: sample.points[1].x,
                    y: sample.points[1].y,
                },
            ],
        };

        let output = self.inner.tick(now_ms, core_sample);
        TouchEngineOutput {
            events: output.events.map(|item| item.map(map_event)),
        }
    }
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
