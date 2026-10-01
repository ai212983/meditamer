//! Host-executable publish-ordering tests for the real analog-clock
//! presentation decision table.
//!
//! Drives the unmodified product model
//! (`products/meditamer/src/firmware/ui/screen/analog_clock/model.rs`,
//! re-exported as `ui_shell_host_harness::analog_clock_model`). The
//! firmware `push_published_frame` consumes `resolve_clock_publish` for
//! its Clean/full-merge branch, so these tests prove the production
//! ordering contract on the real code, not a mirror: the settled staging
//! bits must land in the retained canvas before minute deduplication may
//! advance, for Clean frames as well as Fast ones.
//!
//! Regression: the Clean branch used to set the full-refresh reason and
//! confirm the minute without publishing the canvas. The first frame is
//! always Clean, so the merged repaint only ever covered the initial
//! 0xD8 loading fill (white on the binary panel) while `mark_published`
//! discarded the finished staging -- a permanently blank clock.

use ui_shell_host_harness::analog_clock_model::{
    self, resolve_clock_publish, ClockPublishDecision, MinuteIntent, MinuteTracker, TimeObservation,
};

/// Local wall time under test, in seconds since the local epoch.
fn t(hour: u32, minute: u32, second: u32) -> u32 {
    hour * 3_600 + minute * 60 + second
}

fn intent_of(obs: TimeObservation) -> MinuteIntent {
    match obs {
        TimeObservation::Target { intent, .. } => intent,
        TimeObservation::Duplicate => panic!("expected a fresh-minute target"),
    }
}

#[test]
fn first_frame_clean_publishes_before_confirm() {
    // The first observed minute is always Clean; with the canvas copy
    // landed and Clean allowed, the decision must request the full merge
    // and confirm -- never confirm an unpublished frame.
    let mut tracker = MinuteTracker::new();
    let intent = intent_of(tracker.observe(t(9, 1, 0), 1_000));
    assert_eq!(intent, MinuteIntent::Clean);
    assert_eq!(
        resolve_clock_publish(intent, false, true, true),
        ClockPublishDecision::RequestCleanAndConfirm
    );
}

#[test]
fn clean_canvas_copy_failure_holds_without_confirm() {
    // A Clean frame whose staging bits did not reach the canvas must stay
    // staged for retry: no Clean request, no minute-dedup advance.
    assert_eq!(
        resolve_clock_publish(MinuteIntent::Clean, false, true, false),
        ClockPublishDecision::Hold
    );
}

#[test]
fn clean_without_allowance_holds() {
    // Clean not allowed this cycle: hold even though the frame is Clean.
    assert_eq!(
        resolve_clock_publish(MinuteIntent::Clean, false, false, false),
        ClockPublishDecision::Hold
    );
    assert_eq!(
        resolve_clock_publish(MinuteIntent::Clean, true, false, false),
        ClockPublishDecision::Hold
    );
}

#[test]
fn fast_confirms_only_after_canvas_landing() {
    assert_eq!(
        resolve_clock_publish(MinuteIntent::Fast, false, true, true),
        ClockPublishDecision::ConfirmFast
    );
    assert_eq!(
        resolve_clock_publish(MinuteIntent::Fast, false, true, false),
        ClockPublishDecision::Hold
    );
}

#[test]
fn clean_wins_over_co_ready_fast() {
    // A Fast frame with another Clean request already pending takes the
    // full-merge path once the canvas landed.
    assert_eq!(
        resolve_clock_publish(MinuteIntent::Fast, true, true, true),
        ClockPublishDecision::RequestCleanAndConfirm
    );
    assert_eq!(
        resolve_clock_publish(MinuteIntent::Fast, true, true, false),
        ClockPublishDecision::Hold
    );
}

#[test]
fn failed_canvas_copy_never_confirms_any_intent() {
    // Whatever the intent or pending state, an unlanded canvas copy must
    // never advance deduplication.
    for intent in [MinuteIntent::Clean, MinuteIntent::Fast] {
        for clean_pending in [false, true] {
            assert_eq!(
                resolve_clock_publish(intent, clean_pending, true, false),
                ClockPublishDecision::Hold,
                "intent={intent:?} clean_pending={clean_pending}",
            );
        }
    }
}

#[test]
fn minute_walk_first_clean_then_fast() {
    // End to end across the policy/presentation boundary: the first minute
    // publishes Clean, the ordinary successor publishes Fast, and a
    // duplicate read inside a published minute targets nothing.
    let mut tracker = MinuteTracker::new();

    let intent = intent_of(tracker.observe(t(9, 1, 0), 1_000));
    assert_eq!(intent, MinuteIntent::Clean);
    assert_eq!(
        resolve_clock_publish(intent, false, true, true),
        ClockPublishDecision::RequestCleanAndConfirm
    );
    tracker.published(analog_clock_model::epoch_minute(t(9, 1, 0)), 1_500);

    let intent = intent_of(tracker.observe(t(9, 2, 0), 61_000));
    assert_eq!(intent, MinuteIntent::Fast);
    assert_eq!(
        resolve_clock_publish(intent, false, true, true),
        ClockPublishDecision::ConfirmFast
    );
    tracker.published(analog_clock_model::epoch_minute(t(9, 2, 0)), 61_500);

    assert_eq!(
        tracker.observe(t(9, 2, 30), 91_000),
        TimeObservation::Duplicate
    );
}
