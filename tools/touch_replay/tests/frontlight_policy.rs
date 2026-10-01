//! Deterministic host tests for the Meditamer default-frontlight WAKE policy.
//! Shadows the dependency-free policy module plus the real GPIO36
//! classifier, so generation/timestamp plumbing is exercised end to end
//! without hardware.

#[path = "../../../products/meditamer/src/firmware/display/frontlight_policy.rs"]
mod frontlight_policy;
#[path = "../../../products/meditamer/src/firmware/input/gpio36.rs"]
mod gpio36;

use frontlight_policy::{
    application_deadline, application_due, arm_press, check_hold, classify_release,
    fade_deadline_ms, hold_deadline_ms, needs_level_apply, target_level_for_elapsed, FadeSpec,
    FrontlightCalibrationSession, HoldOutcome, PressArm, ReleaseOutcome, WakePress,
    FRONTLIGHT_APPLY_RETRY_MS, FRONTLIGHT_BASELINE_OFF, FRONTLIGHT_CALIBRATION_INITIAL,
    WAKE_LONG_PRESS_MS,
};
use gpio36::{Gpio36Action, Gpio36Classifier, Gpio36Mode};

const SPEC_ON: FadeSpec = FadeSpec {
    max: 63,
    hold_ms: 3_000,
    fade_ms: 2_000,
    baseline: FRONTLIGHT_CALIBRATION_INITIAL,
};
const SPEC_OFF: FadeSpec = FadeSpec {
    max: 63,
    hold_ms: 3_000,
    fade_ms: 2_000,
    baseline: FRONTLIGHT_BASELINE_OFF,
};

/// Full press/release round trip through the real classifier: ButtonOnly
/// mode accepts immediately with a source timestamp and generation.
fn classified_press_at(classifier: &mut Gpio36Classifier, t_ms: u64) -> WakePress {
    match classifier.on_asserted(t_ms, Gpio36Mode::ButtonOnly) {
        Some(Gpio36Action::WakeButtonPressed {
            t_ms: edge,
            generation,
        }) => {
            assert_eq!(edge, t_ms);
            let snapshot = classifier.snapshot();
            assert_eq!(snapshot.generation, generation);
            assert!(snapshot.active);
            match arm_press(t_ms, generation, snapshot.generation, snapshot.active) {
                PressArm::Armed(press) => press,
                PressArm::IgnoredStale => panic!("fresh press must arm"),
            }
        }
        other => panic!("expected press, got {:?}", other),
    }
}

fn classified_release_at(
    classifier: &mut Gpio36Classifier,
    t_ms: u64,
) -> (u64, u32, gpio36::WakeHoldSnapshot) {
    let action = classifier.on_released(t_ms);
    let snapshot = classifier.snapshot();
    assert!(!snapshot.active, "production publishes release as inactive");
    match action {
        Some(Gpio36Action::WakeButtonReleased {
            t_ms: edge,
            generation,
        }) => (edge, generation, snapshot),
        other => panic!("expected release, got {:?}", other),
    }
}

#[test]
fn hold_boundary_is_700ms() {
    assert_eq!(WAKE_LONG_PRESS_MS, 700);
}

#[test]
fn release_at_699ms_is_short() {
    let mut classifier = Gpio36Classifier::new();
    let press = classified_press_at(&mut classifier, 1_000);
    let (released_ms, generation, snapshot) = classified_release_at(&mut classifier, 1_699);
    // A same-generation inactive snapshot means released, not cancelled.
    assert_eq!(
        check_hold(Some(press), 1_700, snapshot.generation, snapshot.active),
        HoldOutcome::Wait
    );
    assert_eq!(
        classify_release(
            press,
            released_ms,
            generation,
            snapshot.generation,
            snapshot.active
        ),
        ReleaseOutcome::ShortFlash
    );
}

#[test]
fn hold_at_700ms_is_long_while_held() {
    let mut classifier = Gpio36Classifier::new();
    let press = classified_press_at(&mut classifier, 1_000);
    let snapshot = classifier.snapshot();
    assert_eq!(
        check_hold(Some(press), 1_700, snapshot.generation, snapshot.active),
        HoldOutcome::OpenCalibration
    );
}

#[test]
fn release_at_700ms_before_deadline_service_is_late_long() {
    let mut classifier = Gpio36Classifier::new();
    let press = classified_press_at(&mut classifier, 1_000);
    let (released_ms, generation, snapshot) = classified_release_at(&mut classifier, 1_700);
    // The hold service must leave the same-generation inactive press for its
    // queued release edge rather than dropping or acting on it.
    assert_eq!(
        check_hold(Some(press), 1_700, snapshot.generation, snapshot.active),
        HoldOutcome::Wait
    );
    assert_eq!(
        classify_release(
            press,
            released_ms,
            generation,
            snapshot.generation,
            snapshot.active
        ),
        ReleaseOutcome::LateLong
    );
}

#[test]
fn ten_second_hold_opens_calibration_once_and_release_is_consumed() {
    let mut classifier = Gpio36Classifier::new();
    let mut press = classified_press_at(&mut classifier, 1_000);
    let snapshot = classifier.snapshot();

    assert_eq!(
        check_hold(Some(press), 11_000, snapshot.generation, snapshot.active),
        HoldOutcome::OpenCalibration
    );
    // The display marks the hold fired and keeps it until release.
    press.long_fired = true;
    assert_eq!(
        check_hold(Some(press), 11_500, snapshot.generation, snapshot.active),
        HoldOutcome::Wait
    );
    // The later physical release is consumed: no short flash after a long.
    let (released_ms, generation, snapshot) = classified_release_at(&mut classifier, 12_000);
    assert_eq!(
        classify_release(
            press,
            released_ms,
            generation,
            snapshot.generation,
            snapshot.active
        ),
        ReleaseOutcome::ConsumedLong
    );
}

#[test]
fn press_delivered_after_release_still_arms_for_the_queued_release() {
    let mut classifier = Gpio36Classifier::new();
    let Some(Gpio36Action::WakeButtonPressed {
        t_ms: pressed_ms,
        generation,
    }) = classifier.on_asserted(1_000, Gpio36Mode::ButtonOnly)
    else {
        panic!("expected press");
    };
    let (released_ms, release_generation, snapshot) = classified_release_at(&mut classifier, 1_100);

    let PressArm::Armed(press) =
        arm_press(pressed_ms, generation, snapshot.generation, snapshot.active)
    else {
        panic!("same-generation delayed press must arm");
    };
    assert_eq!(
        check_hold(Some(press), 1_700, snapshot.generation, snapshot.active),
        HoldOutcome::Wait
    );
    assert_eq!(
        classify_release(
            press,
            released_ms,
            release_generation,
            snapshot.generation,
            snapshot.active,
        ),
        ReleaseOutcome::ShortFlash
    );
}

#[test]
fn shared_mode_uses_physical_assertion_timestamp() {
    let mut classifier = Gpio36Classifier::new();
    assert_eq!(
        classifier.on_asserted(100, Gpio36Mode::SharedWithTouch),
        None
    );
    for now_ms in [108, 132, 164, 195] {
        assert_eq!(classifier.observe_touch_probe(now_ms, false), None);
    }
    assert_eq!(
        classifier.observe_touch_probe(196, false),
        Some(Gpio36Action::WakeButtonPressed {
            t_ms: 100,
            generation: 1,
        })
    );
}

#[test]
fn cancelled_hold_never_opens_calibration_or_acts() {
    let mut classifier = Gpio36Classifier::new();
    let press = classified_press_at(&mut classifier, 1_000);
    let stale_generation = press.generation;

    // Fault/suspend cancellation while still held.
    classifier.cancel_accepted();
    gpio36::sync_wake_snapshot(&classifier);
    let snapshot = gpio36::load_wake_snapshot();
    assert_ne!(snapshot.generation, stale_generation);
    assert!(!snapshot.active);

    // Deadline servicing drops the stale hold instead of opening calibration.
    assert_eq!(
        check_hold(Some(press), 1_700, snapshot.generation, snapshot.active),
        HoldOutcome::DropStale
    );
    // A release for the ended hold is ignored, not flashed.
    assert_eq!(
        classify_release(
            press,
            1_100,
            stale_generation,
            snapshot.generation,
            snapshot.active
        ),
        ReleaseOutcome::Ignored
    );
    // A press edge armed before the cancellation never arms afterwards.
    assert_eq!(
        arm_press(
            1_000,
            stale_generation,
            snapshot.generation,
            snapshot.active
        ),
        PressArm::IgnoredStale
    );
}

#[test]
fn superseded_generation_is_stale_even_while_active() {
    let mut classifier = Gpio36Classifier::new();
    let first = classified_press_at(&mut classifier, 1_000);
    // A second acceptance supersedes the first while staying active.
    classifier.on_released(1_100);
    let _second = classified_press_at(&mut classifier, 2_000);
    let snapshot = classifier.snapshot();
    assert!(snapshot.active);
    assert_ne!(snapshot.generation, first.generation);

    assert_eq!(
        check_hold(Some(first), 1_700, snapshot.generation, snapshot.active),
        HoldOutcome::DropStale
    );
    assert_eq!(
        classify_release(
            first,
            1_100,
            first.generation,
            snapshot.generation,
            snapshot.active
        ),
        ReleaseOutcome::Ignored
    );
}

#[test]
fn release_generation_mismatch_is_ignored() {
    let press = WakePress::new(1_000, 7);
    assert_eq!(
        classify_release(press, 1_100, 8, 7, true),
        ReleaseOutcome::Ignored
    );
}

#[test]
fn release_without_armed_press_is_ignored() {
    let press = WakePress::new(1_000, 7);
    // No armed press on the display side is modelled by a mismatched
    // snapshot: nothing to act on.
    assert_eq!(
        classify_release(press, 1_100, 7, 99, false),
        ReleaseOutcome::Ignored
    );
    assert_eq!(check_hold(None, 1_700, 7, true), HoldOutcome::Wait);
}

#[test]
fn calibration_starts_at_8_when_baseline_is_off_and_preserves_an_active_baseline() {
    assert_eq!(FRONTLIGHT_BASELINE_OFF, 0);
    assert_eq!(FRONTLIGHT_CALIBRATION_INITIAL, 8);
    assert_eq!(FrontlightCalibrationSession::new(0).original, 0);
    assert_eq!(FrontlightCalibrationSession::new(0).preview, 8);
    assert_eq!(FrontlightCalibrationSession::new(27).original, 27);
    assert_eq!(FrontlightCalibrationSession::new(27).preview, 27);
}

#[test]
fn fade_holds_then_settles_at_baseline_8() {
    assert_eq!(target_level_for_elapsed(SPEC_ON, 0), (63, false));
    assert_eq!(target_level_for_elapsed(SPEC_ON, 2_999), (63, false));
    assert_eq!(target_level_for_elapsed(SPEC_ON, 3_000), (63, false));
    assert_eq!(target_level_for_elapsed(SPEC_ON, 4_000), (35, false));
    assert_eq!(target_level_for_elapsed(SPEC_ON, 4_999), (8, false));
    assert_eq!(target_level_for_elapsed(SPEC_ON, 5_000), (8, true));
    assert_eq!(target_level_for_elapsed(SPEC_ON, 60_000), (8, true));

    // Integer fade steps never rise and never drop below the baseline.
    let mut previous = 63u8;
    let mut step = 0u64;
    while step <= 2_000 {
        let (level, done) = target_level_for_elapsed(SPEC_ON, 3_000 + step);
        assert!(level <= previous, "fade rose at step {}", step);
        assert!(level >= 8, "fade underran baseline at step {}", step);
        assert_eq!(done, 3_000 + step >= 5_000);
        previous = level;
        step += 17;
    }
}

#[test]
fn fade_to_zero_matches_original_max_to_off_shape() {
    assert_eq!(target_level_for_elapsed(SPEC_OFF, 0), (63, false));
    assert_eq!(target_level_for_elapsed(SPEC_OFF, 2_999), (63, false));
    // Original formula: (63 * fade_remaining) / 2000.
    assert_eq!(target_level_for_elapsed(SPEC_OFF, 4_000), (31, false));
    assert_eq!(target_level_for_elapsed(SPEC_OFF, 5_000), (0, true));
}

#[test]
fn no_terminal_deadline_spin_at_zero_or_baseline() {
    let now = 90_000u64;
    // Idle (no cycle): no deadline at any recorded level.
    assert_eq!(fade_deadline_ms(SPEC_ON, None, 0, now), None);
    assert_eq!(fade_deadline_ms(SPEC_ON, None, 8, now), None);
    assert_eq!(fade_deadline_ms(SPEC_OFF, None, 0, now), None);
    assert_eq!(fade_deadline_ms(SPEC_OFF, None, 63, now), None);
    // Settled cycle (fade window passed, level at floor): no deadline.
    assert_eq!(fade_deadline_ms(SPEC_ON, Some(0), 8, 5_000), None);
    assert_eq!(fade_deadline_ms(SPEC_ON, Some(0), 8, 60_000), None);
    assert_eq!(fade_deadline_ms(SPEC_OFF, Some(0), 0, 5_000), None);
    // Exactly one settling pass when the window passed but the recorded
    // level has not caught up; afterwards the cycle clears and the
    // deadline is None per the cases above.
    assert_eq!(fade_deadline_ms(SPEC_ON, Some(0), 63, 60_000), Some(60_000));
}

#[test]
fn failed_application_waits_for_retry_and_reapplies_uncertain_equal_level() {
    assert_eq!(FRONTLIGHT_APPLY_RETRY_MS, 250);
    assert!(!application_due(Some(1_250), 1_249));
    assert!(application_due(Some(1_250), 1_250));
    assert!(application_due(None, 0));

    // A failed max attempt may leave the rail uncertain while the last
    // confirmed and newly desired levels are both zero. It still needs a
    // confirmed zero write/off sequence at the retry deadline.
    assert!(needs_level_apply(0, 0, true));
    assert!(!needs_level_apply(0, 0, false));

    // A past fade deadline must not win and hot-loop before the retry.
    assert_eq!(application_deadline(Some(1_000), Some(1_250)), Some(1_250));
    assert_eq!(application_deadline(Some(1_000), None), Some(1_000));
}

#[test]
fn fade_deadlines_cover_hold_and_steps() {
    // Steady hold: single wakeup just after the fade starts.
    assert_eq!(
        fade_deadline_ms(SPEC_ON, Some(1_000), 63, 1_000),
        Some(4_001)
    );
    // Mid-fade progress schedules a wakeup no later than the fade end.
    let mid = fade_deadline_ms(SPEC_ON, Some(0), 51, 3_500);
    assert!(mid.is_some_and(|at| at <= 5_000));
    // A level already at the baseline mid-window only needs the settle pass.
    assert_eq!(fade_deadline_ms(SPEC_ON, Some(0), 8, 3_500), Some(5_000));
}

#[test]
fn hold_deadline_comes_from_the_source_press_timestamp() {
    // Armed and unfired: deadline is press + 700, even if the display only
    // serviced the edge late (delayed servicing still decides the hold).
    assert_eq!(
        hold_deadline_ms(Some(WakePress::new(1_000, 1))),
        Some(1_700)
    );
    // Fired holds are owned by the release path: no deadline.
    assert_eq!(
        hold_deadline_ms(Some(WakePress {
            pressed_ms: 1_000,
            generation: 1,
            long_fired: true,
        })),
        None
    );
    assert_eq!(hold_deadline_ms(None), None);
}

#[test]
fn wake_snapshot_round_trips_through_the_static() {
    // The only test in this binary touching the shared static; everything
    // else reads the classifier-local snapshot, so parallel tests cannot
    // interfere.
    let mut classifier = Gpio36Classifier::new();
    let action = classifier.on_asserted(500, Gpio36Mode::ButtonOnly);
    let Some(Gpio36Action::WakeButtonPressed { generation, .. }) = action else {
        panic!("expected press");
    };
    gpio36::sync_wake_snapshot(&classifier);
    assert_eq!(
        gpio36::load_wake_snapshot(),
        gpio36::WakeHoldSnapshot {
            generation,
            active: true,
        }
    );

    let release = classifier.on_released(600);
    assert!(matches!(
        release,
        Some(Gpio36Action::WakeButtonReleased { .. })
    ));
    gpio36::sync_wake_snapshot(&classifier);
    let snapshot = gpio36::load_wake_snapshot();
    assert_eq!(snapshot.generation, generation);
    assert!(!snapshot.active);
}
