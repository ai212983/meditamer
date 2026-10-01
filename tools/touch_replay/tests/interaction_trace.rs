#![allow(dead_code)]

mod types {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum TouchEventKind {
        Down,
        Move,
        Up,
        Tap,
        LongPress,
        Swipe,
        Cancel,
    }
}

#[path = "../../../products/meditamer/src/firmware/touch/interaction_trace.rs"]
mod interaction_trace;

use interaction_trace::{next_frame_id, FrameCorrelation, GestureCorrelation};
use types::TouchEventKind;

fn frame(contact_id: u32) -> FrameCorrelation {
    FrameCorrelation {
        contact_id,
        frame_id: next_frame_id(),
    }
}

#[test]
fn first_touch_after_reset_starts_a_new_gesture() {
    let mut correlation = GestureCorrelation::default();
    let first = frame(41);
    correlation.observe_frame(first);
    assert_eq!(correlation.event_id(TouchEventKind::Down, Some(first)), 41);

    let reset = correlation.reset_boundary();
    assert_eq!(reset.id, 41);
    assert_eq!(reset.generation, 1);
    assert_eq!(correlation.event_id(TouchEventKind::Cancel, None), 41);
    correlation.finish_batch(true);

    // Suppressed frames are not admitted to correlation state.
    let second = frame(42);
    correlation.observe_frame(second);
    assert_eq!(correlation.event_id(TouchEventKind::Down, Some(second)), 42);
}

#[test]
fn suppressed_contact_cannot_become_the_next_debounce_origin() {
    let mut correlation = GestureCorrelation::default();

    // A reset gate drops this contact and its release, so the pipeline must
    // not call `observe_frame` for either sample.
    let fresh = frame(52);
    correlation.observe_frame(fresh);
    correlation.reconcile_engine(false);

    assert_eq!(correlation.event_id(TouchEventKind::Down, None), 52);
}

#[test]
fn delayed_cancel_keeps_the_old_gesture_when_new_frames_arrive() {
    let mut correlation = GestureCorrelation::default();
    let old = frame(7);
    correlation.observe_frame(old);
    assert_eq!(correlation.event_id(TouchEventKind::Down, Some(old)), 7);
    correlation.reset_boundary();

    // A held physical contact may still be sampled while suppression waits for
    // release. It must not replace the gesture cancelled by the reset.
    correlation.observe_frame(frame(7));
    // Even a subsequent contact observation cannot steal the in-flight
    // gesture's cancellation identity.
    correlation.observe_frame(frame(8));
    assert_eq!(correlation.reset_boundary().id, 7);
    assert_eq!(correlation.event_id(TouchEventKind::Cancel, None), 7);
    correlation.finish_batch(true);

    let fresh = frame(8);
    correlation.observe_frame(fresh);
    assert_eq!(correlation.event_id(TouchEventKind::Down, Some(fresh)), 8);
}

#[test]
fn frame_ids_do_not_repeat_when_correlation_state_restarts() {
    let first = frame(1);
    let second = frame(2);
    assert_ne!(first.frame_id, 0);
    assert_ne!(second.frame_id, 0);
    assert_ne!(first.frame_id, second.frame_id);

    let mut restarted = GestureCorrelation::default();
    restarted.observe_frame(second);
    assert_eq!(restarted.event_id(TouchEventKind::Down, Some(second)), 2);
}

#[test]
fn timer_promoted_down_uses_the_debounce_origin_contact() {
    let mut correlation = GestureCorrelation::default();
    let origin = frame(12);
    correlation.observe_frame(origin);
    // The HSM is still in debounce-down after the originating raw frame.
    correlation.reconcile_engine(false);

    // A grace-period timer promotes the press without a current raw frame.
    assert_eq!(correlation.event_id(TouchEventKind::Down, None), 12);
}

#[test]
fn bounce_replacement_before_promotion_keeps_the_first_contact() {
    let mut correlation = GestureCorrelation::default();
    let origin = frame(21);
    correlation.observe_frame(origin);
    correlation.reconcile_engine(false);

    // A zero/replacement pair can be normalized as one pending debounce.
    correlation.observe_frame(frame(0));
    correlation.observe_frame(frame(22));
    correlation.reconcile_engine(false);
    assert_eq!(correlation.event_id(TouchEventKind::Down, None), 21);
}

#[test]
fn aborted_debounce_does_not_leak_into_the_next_contact() {
    let mut correlation = GestureCorrelation::default();
    correlation.observe_frame(frame(31));
    correlation.reconcile_engine(false);

    // HSM debounce abort returned to idle without an event.
    correlation.reconcile_engine(true);
    assert_eq!(correlation.active_or_last_id(), 0);
    let next = frame(32);
    correlation.observe_frame(next);
    correlation.reconcile_engine(false);
    assert_eq!(correlation.event_id(TouchEventKind::Down, None), 32);
}

#[test]
fn cancellation_before_down_keeps_the_pending_contact() {
    let mut correlation = GestureCorrelation::default();
    correlation.observe_frame(frame(41));
    correlation.reconcile_engine(false);

    assert_eq!(correlation.event_id(TouchEventKind::Cancel, None), 41);
    correlation.finish_batch(true);
    correlation.reconcile_engine(true);
}
