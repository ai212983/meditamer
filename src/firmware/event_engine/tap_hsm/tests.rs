use super::super::features::{
    LSM6_TAP_SRC_SINGLE_TAP_BIT, LSM6_TAP_SRC_TAP_EVENT_BIT, LSM6_TAP_SRC_Z_BIT,
};
use super::*;

fn candidate_frame(now_ms: u64) -> SensorFrame {
    SensorFrame {
        now_ms,
        tap_src: LSM6_TAP_SRC_Z_BIT | LSM6_TAP_SRC_SINGLE_TAP_BIT | LSM6_TAP_SRC_TAP_EVENT_BIT,
        int1: true,
        ..SensorFrame::default()
    }
}

#[test]
fn third_candidate_triggers_after_sequence_progression() {
    let mut engine = EventEngine::default();

    let first = engine.tick(candidate_frame(1_000));
    assert!(!first.actions.contains_backlight_trigger());

    let second = engine.tick(candidate_frame(1_200));
    assert!(!second.actions.contains_backlight_trigger());
    assert_eq!(second.trace.state_id, EngineStateId::TapSeq1);
    assert_eq!(second.trace.reject_reason, RejectReason::None);

    let third = engine.tick(candidate_frame(1_400));
    assert!(third.actions.contains_backlight_trigger());
    assert_eq!(third.trace.state_id, EngineStateId::TapSeq2);
    assert_eq!(third.trace.reject_reason, RejectReason::None);
}

#[test]
fn long_gap_in_tap_seq2_rejects_without_triggering() {
    let mut engine = EventEngine::default();

    let _ = engine.tick(candidate_frame(1_000));
    let _ = engine.tick(candidate_frame(1_200));
    let late = engine.tick(candidate_frame(2_200));

    assert!(!late.actions.contains_backlight_trigger());
    assert_eq!(late.trace.state_id, EngineStateId::TapSeq2);
    assert_eq!(late.trace.reject_reason, RejectReason::GapTooLong);
}
