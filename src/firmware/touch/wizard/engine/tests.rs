use super::super::types::{TouchEvent, TouchEventKind, TouchSwipeDirection};
use super::events::pending_release_matches_swipe;
use super::{PendingSwipeRelease, SwipePoint};

fn pending_release() -> PendingSwipeRelease {
    PendingSwipeRelease {
        t_ms: 120,
        start: SwipePoint { x: 100, y: 200 },
        end: SwipePoint { x: 140, y: 202 },
        duration_ms: 120,
        move_count: 3,
        max_travel_px: 72,
        release_debounce_ms: 56,
        dropout_count: 1,
    }
}

#[test]
fn pending_release_matches_same_swipe_even_if_end_differs() {
    let pending = pending_release();
    let swipe = TouchEvent {
        kind: TouchEventKind::Swipe(TouchSwipeDirection::Right),
        t_ms: pending.t_ms,
        x: 220,
        y: 206,
        start_x: pending.start.x as u16,
        start_y: pending.start.y as u16,
        duration_ms: pending.duration_ms,
        touch_count: 0,
        move_count: pending.move_count,
        max_travel_px: pending.max_travel_px,
        release_debounce_ms: pending.release_debounce_ms,
        dropout_count: pending.dropout_count,
    };

    assert!(pending_release_matches_swipe(pending, swipe));
}

#[test]
fn pending_release_rejects_unrelated_swipe() {
    let pending = pending_release();
    let swipe = TouchEvent {
        kind: TouchEventKind::Swipe(TouchSwipeDirection::Right),
        t_ms: pending.t_ms + 1,
        x: 220,
        y: 206,
        start_x: pending.start.x as u16,
        start_y: pending.start.y as u16,
        duration_ms: pending.duration_ms,
        touch_count: 0,
        move_count: pending.move_count,
        max_travel_px: pending.max_travel_px,
        release_debounce_ms: pending.release_debounce_ms,
        dropout_count: pending.dropout_count,
    };

    assert!(!pending_release_matches_swipe(pending, swipe));
}
