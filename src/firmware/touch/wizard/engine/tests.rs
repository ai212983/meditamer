use super::super::super::types::{TouchEvent, TouchEventKind, TouchSwipeDirection};
use super::events::pending_release_matches_swipe;
use super::flow::PrecisionMenuAction;
use super::{
    PendingSwipeRelease, SwipePoint, TapObservation, TestCoordinateMode, TouchCalibration,
    TouchCalibrationWizard, WizardPhase,
};

fn touch_event(kind: TouchEventKind, x: u16, y: u16) -> TouchEvent {
    TouchEvent {
        kind,
        t_ms: 100,
        x,
        y,
        contact_x: x,
        contact_y: y,
        start_x: x,
        start_y: y,
        duration_ms: 0,
        touch_count: 1,
        move_count: 0,
        max_travel_px: 0,
        release_debounce_ms: 0,
        dropout_count: 0,
    }
}

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
        contact_x: pending.start.x as u16,
        contact_y: pending.start.y as u16,
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
        contact_x: pending.start.x as u16,
        contact_y: pending.start.y as u16,
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

#[test]
fn wizard_starts_on_precision_menu() {
    let mut wizard = TouchCalibrationWizard::new(true);
    assert_eq!(wizard.phase, WizardPhase::PrecisionMenu);
    assert_eq!(wizard.test_mode, TestCoordinateMode::Calibrated);
    assert!(wizard.calibration.is_none());
}

#[test]
fn recognized_swipe_outside_target_is_recorded_and_advances_diagnostic() {
    let mut wizard = TouchCalibrationWizard::new(true);
    wizard.phase = WizardPhase::SwipeRight;

    let swipe = TouchEvent {
        kind: TouchEventKind::Swipe(TouchSwipeDirection::Right),
        t_ms: 680,
        x: 506,
        y: 287,
        contact_x: 94,
        contact_y: 318,
        start_x: 94,
        start_y: 318,
        duration_ms: 680,
        touch_count: 0,
        move_count: 18,
        max_travel_px: 413,
        release_debounce_ms: 84,
        dropout_count: 1,
    };

    assert!(wizard.on_swipe_event(swipe, TouchSwipeDirection::Right));
    assert_eq!(wizard.swipe_case_index, 1);
    assert_eq!(wizard.swipe_case_attempts, 1);
    assert_eq!(wizard.swipe_case_failed, 1);
}

#[test]
fn four_corner_calibration_maps_observed_edges_to_targets() {
    let mut wizard = TouchCalibrationWizard::new(true);
    wizard.start_calibration();

    for point in [
        SwipePoint { x: 62, y: 60 },
        SwipePoint { x: 536, y: 60 },
        SwipePoint { x: 536, y: 534 },
        SwipePoint { x: 62, y: 534 },
    ] {
        assert!(wizard.record_calibration_touch(point.x, point.y, 600, 600));
    }

    assert_eq!(wizard.calibration_count(), 4);
    assert!(wizard.calibration.is_some());
    assert!(wizard.calibration_pending_return);

    let calibrated = wizard.calibrated_event(touch_event(TouchEventKind::Down, 62, 60), 600, 600);
    assert_eq!((calibrated.x, calibrated.y), (52, 52));
}

#[test]
fn calibration_maps_first_contact_independently_from_debounced_position() {
    let mut wizard = TouchCalibrationWizard::new(true);
    wizard.calibration = TouchCalibration::from_observations(calibration_observations());
    let mut event = touch_event(TouchEventKind::Down, 304, 209);
    event.contact_x = 62;
    event.contact_y = 60;
    event.start_x = 70;
    event.start_y = 68;

    let calibrated = wizard.calibrated_event(event, 600, 600);

    assert_eq!((calibrated.contact_x, calibrated.contact_y), (52, 52));
    assert_eq!((calibrated.start_x, calibrated.start_y), (60, 60));
    assert_ne!(
        (calibrated.x, calibrated.y),
        (calibrated.contact_x, calibrated.contact_y)
    );
}

#[test]
fn starting_recalibration_discards_the_previous_transform() {
    let mut wizard = TouchCalibrationWizard::new(true);
    wizard.calibration = TouchCalibration::from_observations(calibration_observations());
    wizard.last_test_touch = Some(super::TestTouch {
        raw: SwipePoint { x: 62, y: 60 },
        calibrated: SwipePoint { x: 52, y: 52 },
    });

    wizard.start_calibration();

    assert!(wizard.calibration.is_none());
    assert!(wizard.last_test_touch.is_none());
    assert_eq!(wizard.calibration_count(), 0);
}

#[test]
fn calibration_targets_are_margin_pixels_from_inclusive_edges() {
    assert_eq!(
        TouchCalibrationWizard::calibration_targets(600, 600),
        [
            SwipePoint { x: 52, y: 52 },
            SwipePoint { x: 547, y: 52 },
            SwipePoint { x: 547, y: 547 },
            SwipePoint { x: 52, y: 547 },
        ]
    );
}

#[test]
fn precision_menu_actions_match_two_rows() {
    let wizard = TouchCalibrationWizard::new(true);

    assert_eq!(
        wizard.precision_menu_action(100, 250, 600, 600),
        Some(PrecisionMenuAction::Calibrate)
    );
    assert_eq!(
        wizard.precision_menu_action(500, 250, 600, 600),
        Some(PrecisionMenuAction::Test)
    );
    assert_eq!(
        wizard.precision_menu_action(300, 350, 600, 600),
        Some(PrecisionMenuAction::Continue)
    );
}

#[test]
fn continue_from_precision_menu_starts_swipes() {
    let mut wizard = TouchCalibrationWizard::new(true);

    assert!(wizard.on_continue_button(120));
    assert_eq!(wizard.phase, WizardPhase::SwipeRight);
}

fn calibration_observations() -> [Option<TapObservation>; 4] {
    let targets = TouchCalibrationWizard::calibration_targets(600, 600);
    let observed = [
        SwipePoint { x: 62, y: 60 },
        SwipePoint { x: 536, y: 60 },
        SwipePoint { x: 536, y: 534 },
        SwipePoint { x: 62, y: 534 },
    ];
    core::array::from_fn(|index| {
        Some(TapObservation {
            target: targets[index],
            observed: observed[index],
        })
    })
}
