use super::super::config::{TapThresholdConfig, TapWeightConfig, TripleTapConfig};
use super::*;

fn cfg() -> TripleTapConfig {
    TripleTapConfig {
        enabled: true,
        min_gap_ms: 55,
        max_gap_ms: 700,
        last_max_gap_ms: 900,
        cooldown_ms: 900,
        debounce_ms: 110,
        seq_finish_debounce_ms: 55,
        gyro_veto_hold_ms: 180,
        thresholds: TapThresholdConfig {
            jerk_l1_min: 900,
            jerk_strong_l1_min: 2_600,
            jerk_seq_cont_min: 650,
            prev_jerk_quiet_max: 1_100,
            gyro_l1_swing_max: 14_000,
        },
        weights: TapWeightConfig {
            axis_weight: 30,
            single_tap_weight: 25,
            int1_weight: 15,
            tap_event_weight: 20,
            jerk_axis_weight: 10,
            jerk_only_weight: 35,
            seq_finish_weight: 20,
        },
    }
}

#[test]
fn jerk_and_axis_prefers_largest_delta_axis() {
    let (jerk, axis) = accel_l1_jerk_and_axis(Some((100, 120, 140)), (160, 122, 139));
    assert_eq!(jerk, 63);
    assert_eq!(axis, LSM6_TAP_SRC_X_BIT);
}

#[test]
fn gyro_veto_window_stays_active_for_hold_duration() {
    let config = cfg();
    let frame = SensorFrame {
        now_ms: 1_000,
        gx: 5_000,
        gy: 5_000,
        gz: 4_100,
        ..SensorFrame::default()
    };
    let (_, last_big_gyro) = compute_motion_features(frame, None, 0, None, &config);
    assert_eq!(last_big_gyro, Some(1_000));

    let later = SensorFrame {
        now_ms: 1_100,
        gx: 0,
        gy: 0,
        gz: 0,
        ..frame
    };
    let (features, _) = compute_motion_features(later, None, 0, last_big_gyro, &config);
    assert!(features.gyro_veto_active);

    let much_later = SensorFrame {
        now_ms: 1_250,
        gx: 0,
        gy: 0,
        gz: 0,
        ..frame
    };
    let (features, _) = compute_motion_features(much_later, None, 0, last_big_gyro, &config);
    assert!(!features.gyro_veto_active);
}

/// Ringing after a tap oscillates: it clears every jerk threshold on its peaks
/// and the quiet-before-impulse gate in its dips. Only `tap_src` distinguishes
/// it from a real impact, so no jerk magnitude alone may produce a candidate.
#[test]
fn ringdown_without_tap_src_is_never_a_candidate() {
    let config = cfg();
    for (jerk, prev_jerk) in [(3_393, 905), (29_348, 78), (39_263, 1_000), (2_065, 900)] {
        let ringing = MotionFeatures {
            jerk_l1: jerk,
            prev_jerk_l1: prev_jerk,
            candidate_axis: LSM6_TAP_SRC_Z_BIT,
            ..MotionFeatures::default()
        };

        // Both mid-sequence (where the assist path applies) and from idle.
        for seq_count in [0, 2] {
            let decision = assess_tap_candidate(
                &ringing,
                seq_count,
                LSM6_TAP_SRC_Z_BIT,
                None,
                1_300,
                &config,
            );
            assert!(
                !decision.accepted,
                "jerk={jerk} prev={prev_jerk} seq={seq_count} accepted without tap_src"
            );
            assert_eq!(decision.reason, RejectReason::CandidateWeak);
        }
    }
}

#[test]
fn hardware_tap_is_a_candidate_on_any_axis() {
    let config = cfg();
    // A real tap reported on X while the sequence started on Z: the detector
    // picks a different dominant axis between taps of one deliberate sequence.
    let tap = MotionFeatures {
        tap_axis_mask: LSM6_TAP_SRC_X_BIT,
        has_axis_tap: true,
        has_single_tap: true,
        candidate_axis: LSM6_TAP_SRC_X_BIT,
        jerk_l1: 14_941,
        ..MotionFeatures::default()
    };

    let decision = assess_tap_candidate(&tap, 2, LSM6_TAP_SRC_Z_BIT, None, 1_300, &config);
    assert!(decision.accepted);
    assert_eq!(decision.reason, RejectReason::None);
}

/// The detector reports one physical tap on consecutive samples ~11 ms apart,
/// so the debounce window still has to collapse those into a single candidate.
#[test]
fn repeated_tap_src_within_debounce_window_is_rejected() {
    let config = cfg();
    let features = MotionFeatures {
        tap_axis_mask: LSM6_TAP_SRC_X_BIT,
        has_axis_tap: true,
        has_single_tap: true,
        candidate_axis: LSM6_TAP_SRC_X_BIT,
        jerk_l1: 3_000,
        prev_jerk_l1: 100,
        ..MotionFeatures::default()
    };

    let decision = assess_tap_candidate(&features, 0, 0, Some(1_000), 1_050, &config);
    assert!(!decision.accepted);
    assert_eq!(decision.reason, RejectReason::Debounced);
}
