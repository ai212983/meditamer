pub mod admission;
pub mod config;
mod core;
#[cfg(feature = "wifi-debug-slim-app")]
#[path = "debug_log_stub.rs"]
pub(crate) mod debug_log;
#[cfg(not(feature = "wifi-debug-slim-app"))]
pub(crate) mod debug_log;
pub(crate) mod event_time;
mod imu_activity;
#[cfg(feature = "ui-interaction-trace")]
pub(crate) mod interaction_trace;
pub(crate) mod lvgl_multitouch;
pub(crate) mod merge;
mod normalize;
pub(crate) mod replay;
pub(crate) mod scheduling;
pub mod tasks;
pub(crate) mod types;

use inkplate_tempera::TouchSample as HalTouchSample;
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

    /// Ends an in-flight interaction before its raw contact is released or
    /// replaced -- the touch counterpart of `CaptureGate`'s underlying
    /// `ButtonRecognizer::cancel`. A no-op if nothing is in flight.
    pub(crate) fn cancel(&mut self, now_ms: u64) -> TouchEngineOutput {
        let output = self.inner.cancel(now_ms);
        TouchEngineOutput {
            events: output.events.map(|item| item.map(map_event)),
        }
    }

    /// Release grace and in-flight gestures still need the existing active cadence.
    pub(crate) fn needs_tick(&self) -> bool {
        !self.inner.is_idle() || self.normalizer.has_presence()
    }

    #[cfg(feature = "ui-interaction-trace")]
    pub(crate) fn is_idle(&self) -> bool {
        self.inner.is_idle()
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
        // The pipeline stamps the epoch that owns the HSM, including timers.
        admission_epoch: 0,
        #[cfg(feature = "ui-interaction-trace")]
        trace_id: 0,
        kind: map_kind(event.kind),
        t_ms: event_time::EventTime::new(event.t_ms),
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
