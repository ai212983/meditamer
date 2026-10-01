//! Host-executable cadence tests for the real analog-clock minute policy.
//!
//! Drives the unmodified product model
//! (`products/meditamer/src/firmware/ui/screen/analog_clock/model.rs`,
//! re-exported as `ui_shell_host_harness::analog_clock_model`). Every test
//! exercises `observe` together with `published`, because the wall-minute
//! deadline contract lives in their interaction: `observe` sets the next
//! wall-minute rollover deadline, `published` preserves it (never pushing it
//! out by a minute), and only a successful panel publish advances
//! deduplication.

use ui_shell_host_harness::analog_clock_model::{
    self, MinuteIntent, MinuteTracker, TimeObservation,
};

/// Local wall time under test, in seconds since the local epoch.
fn t(hour: u32, minute: u32, second: u32) -> u32 {
    hour * 3_600 + minute * 60 + second
}

fn target_of(obs: TimeObservation) -> (u64, MinuteIntent) {
    match obs {
        TimeObservation::Target {
            epoch_minute,
            intent,
        } => (epoch_minute, intent),
        TimeObservation::Duplicate => panic!("expected a fresh-minute target"),
    }
}

#[test]
fn catalogue_ids_match_product_contract() {
    assert_eq!(analog_clock_model::ENTRY_NAMESPACE, 1);
    assert_eq!(analog_clock_model::ENTRY_LOCAL, 6);
    assert_eq!(analog_clock_model::SURFACE_ID, 11);
    assert_eq!(analog_clock_model::WIDTH_PX, 600);
    assert_eq!(analog_clock_model::HEIGHT_PX, 600);
}

#[test]
fn ordinary_minute_is_fast_after_first_clean() {
    let mut tracker = MinuteTracker::new();
    let (_, intent) = target_of(tracker.observe(t(9, 1, 0), 1_000));
    assert_eq!(intent, MinuteIntent::Clean);
    tracker.published(analog_clock_model::epoch_minute(t(9, 1, 0)), 1_500);

    let (epoch, intent) = target_of(tracker.observe(t(9, 2, 0), 61_000));
    assert_eq!(epoch, analog_clock_model::epoch_minute(t(9, 2, 0)));
    assert_eq!(intent, MinuteIntent::Fast);
}

#[test]
fn nine_to_ten_boundary_is_clean() {
    let mut tracker = MinuteTracker::new();
    let _ = tracker.observe(t(9, 9, 0), 1_000);
    tracker.published(analog_clock_model::epoch_minute(t(9, 9, 0)), 1_500);

    let (_, intent) = target_of(tracker.observe(t(9, 10, 0), 61_000));
    assert_eq!(intent, MinuteIntent::Clean);
}

#[test]
fn fifty_nine_to_zero_boundary_is_clean() {
    let mut tracker = MinuteTracker::new();
    let _ = tracker.observe(t(8, 59, 0), 1_000);
    tracker.published(analog_clock_model::epoch_minute(t(8, 59, 0)), 1_500);

    let (_, intent) = target_of(tracker.observe(t(9, 0, 0), 61_000));
    assert_eq!(intent, MinuteIntent::Clean);
    let (hour, minute) = analog_clock_model::clock_h_m(t(9, 0, 0));
    assert_eq!((hour, minute), (9, 0));
}

#[test]
fn duplicate_minute_after_publish_is_ignored() {
    let mut tracker = MinuteTracker::new();
    let _ = tracker.observe(t(9, 2, 10), 1_000);
    tracker.published(analog_clock_model::epoch_minute(t(9, 2, 10)), 1_500);

    assert_eq!(
        tracker.observe(t(9, 2, 40), 30_000),
        TimeObservation::Duplicate
    );
}

#[test]
fn reentry_renders_clean_once() {
    // Reentry builds a fresh engine and tracker, so the first observation
    // after navigation is clean whatever the minute.
    let mut tracker = MinuteTracker::new();
    let (_, intent) = target_of(tracker.observe(t(9, 2, 40), 30_000));
    assert_eq!(intent, MinuteIntent::Clean);
}

#[test]
fn skip_over_clean_boundary_recovers_clean() {
    let mut tracker = MinuteTracker::new();
    let _ = tracker.observe(t(9, 8, 0), 1_000);
    tracker.published(analog_clock_model::epoch_minute(t(9, 8, 0)), 1_500);

    // The 9:10 boundary passed unseen: full refresh, not a partial.
    let (_, intent) = target_of(tracker.observe(t(9, 11, 0), 200_000));
    assert_eq!(intent, MinuteIntent::Clean);
}

#[test]
fn skip_without_clean_boundary_stays_fast() {
    let mut tracker = MinuteTracker::new();
    let _ = tracker.observe(t(9, 1, 0), 1_000);
    tracker.published(analog_clock_model::epoch_minute(t(9, 1, 0)), 1_500);

    let (_, intent) = target_of(tracker.observe(t(9, 5, 0), 300_000));
    assert_eq!(intent, MinuteIntent::Fast);
}

#[test]
fn backward_jump_recovers_clean() {
    let mut tracker = MinuteTracker::new();
    let _ = tracker.observe(t(9, 5, 0), 1_000);
    tracker.published(analog_clock_model::epoch_minute(t(9, 5, 0)), 1_500);

    let (_, intent) = target_of(tracker.observe(t(9, 3, 0), 61_000));
    assert_eq!(intent, MinuteIntent::Clean);
}

#[test]
fn failed_publish_is_not_committed() {
    // Observe a new minute but never publish it (the render never reached
    // the panel): the same minute must still retarget instead of reading
    // back as a duplicate.
    let mut tracker = MinuteTracker::new();
    let _ = tracker.observe(t(9, 1, 0), 1_000);
    tracker.published(analog_clock_model::epoch_minute(t(9, 1, 0)), 1_500);

    let (first, _) = target_of(tracker.observe(t(9, 2, 10), 61_000));
    // No published() call: the frame never completed.
    let (second, intent) = target_of(tracker.observe(t(9, 2, 40), 90_000));
    assert_eq!(first, second);
    assert_eq!(intent, MinuteIntent::Fast);
}

#[test]
fn publish_preserves_wall_minute_deadline() {
    // Observe at 59 s: the deadline is the coming rollover, 1 s out.
    let mut tracker = MinuteTracker::new();
    let _ = tracker.observe(t(9, 1, 59), 1_000);
    assert_eq!(tracker.next_poll_due_ms(), 2_000);

    // A fast render finishing before the deadline must not push the next
    // read out by a minute (the old now+61000 drift).
    tracker.published(analog_clock_model::epoch_minute(t(9, 1, 59)), 1_500);
    assert_eq!(tracker.next_poll_due_ms(), 2_000);
}

#[test]
fn publish_after_deadline_is_due_immediately() {
    let mut tracker = MinuteTracker::new();
    let _ = tracker.observe(t(9, 1, 59), 1_000);
    assert_eq!(tracker.next_poll_due_ms(), 2_000);

    // The render finished after its deadline: a fresh read is due now,
    // not one minute out.
    tracker.published(analog_clock_model::epoch_minute(t(9, 1, 59)), 5_000);
    assert_eq!(tracker.next_poll_due_ms(), 5_000);
}

#[test]
fn completed_minute_does_not_drift_sixty_one_seconds() {
    let mut tracker = MinuteTracker::new();
    let _ = tracker.observe(t(9, 2, 5), 7_000);
    // Next rollover is 55 s out.
    assert_eq!(tracker.next_poll_due_ms(), 62_000);

    tracker.published(analog_clock_model::epoch_minute(t(9, 2, 5)), 8_000);
    // Still the wall rollover, not completion + 61 s.
    assert_eq!(tracker.next_poll_due_ms(), 62_000);
    assert!(tracker.next_poll_due_ms() - 8_000 < 61_000);
}
