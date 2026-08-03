pub(crate) mod config;
mod core;
#[cfg(feature = "wifi-debug-slim-app")]
#[path = "debug_log_stub.rs"]
pub(crate) mod debug_log;
#[cfg(not(feature = "wifi-debug-slim-app"))]
pub(crate) mod debug_log;
mod imu_activity;
mod normalize;
pub(crate) mod scheduling;
pub(crate) mod tasks;
pub(crate) mod types;

use crate::drivers::inkplate::TouchSample as HalTouchSample;
use normalize::{NormalizedTouchPoint, NormalizedTouchSample, TouchPresenceNormalizer};

use self::types::{TouchEvent, TouchEventKind, TouchSwipeDirection};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TouchEngineOutput {
    pub(crate) events: [Option<TouchEvent>; 3],
}

pub(crate) struct TouchEngine {
    inner: core::TouchEngine,
    normalizer: TouchPresenceNormalizer,
    last_primary: core::TouchPoint,
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
            normalizer: TouchPresenceNormalizer::new(),
            last_primary: core::TouchPoint::default(),
        }
    }

    pub(crate) fn tick(&mut self, now_ms: u64, sample: HalTouchSample) -> TouchEngineOutput {
        let normalized = NormalizedTouchSample {
            touch_count: sample.touch_count,
            points: [
                NormalizedTouchPoint {
                    x: sample.points[0].x,
                    y: sample.points[0].y,
                },
                NormalizedTouchPoint {
                    x: sample.points[1].x,
                    y: sample.points[1].y,
                },
            ],
            raw: sample.raw,
        };
        let (normalized_count, primary) = self.normalizer.normalize(now_ms, normalized);

        let core_sample = core::TouchSample {
            touch_count: normalized_count,
            points: [
                primary
                    .map(|p| core::TouchPoint { x: p.x, y: p.y })
                    .unwrap_or_default(),
                core::TouchPoint::default(),
            ],
        };
        if normalized_count > 0 {
            self.last_primary = core_sample.points[0];
        }

        let output = self.inner.tick(now_ms, core_sample);
        TouchEngineOutput {
            events: output.events.map(|item| item.map(map_event)),
        }
    }

    pub(crate) fn advance(&mut self, now_ms: u64) -> TouchEngineOutput {
        let (normalized_count, primary) = self.normalizer.advance(now_ms);
        if let Some(point) = primary {
            self.last_primary = core::TouchPoint {
                x: point.x,
                y: point.y,
            };
        }
        let core_sample = core::TouchSample {
            touch_count: normalized_count,
            points: [self.last_primary, core::TouchPoint::default()],
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
        contact_x: event.contact_x,
        contact_y: event.contact_y,
        start_x: event.start_x,
        start_y: event.start_y,
        duration_ms: event.duration_ms,
        touch_count: event.touch_count,
        move_count: event.move_count,
        max_travel_px: event.max_travel_px,
        release_debounce_ms: event.release_debounce_ms,
        dropout_count: event.dropout_count,
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
