use crate::event_engine::{
    config::TripleTapConfig,
    types::{CandidateScore, MotionFeatures, RejectReason, SensorFrame},
};

pub const LSM6_TAP_SRC_Z_BIT: u8 = 0x01;
pub const LSM6_TAP_SRC_Y_BIT: u8 = 0x02;
pub const LSM6_TAP_SRC_X_BIT: u8 = 0x04;
pub const LSM6_TAP_SRC_SINGLE_TAP_BIT: u8 = 0x20;
pub const LSM6_TAP_SRC_TAP_EVENT_BIT: u8 = 0x40;

pub const CAND_SRC_AXIS: u8 = 0x01;
pub const CAND_SRC_SINGLE: u8 = 0x02;
pub const CAND_SRC_INT1: u8 = 0x04;
pub const CAND_SRC_TAP_EVENT: u8 = 0x08;
pub const CAND_SRC_JERK_AXIS: u8 = 0x10;
pub const CAND_SRC_JERK_ONLY: u8 = 0x20;
pub const CAND_SRC_GYRO_VETO: u8 = 0x40;
pub const CAND_SRC_SEQ_ASSIST: u8 = 0x80;

#[derive(Clone, Copy, Debug, Default)]
pub struct CandidateAssessment {
    pub accepted: bool,
    pub source_mask: u8,
    pub score: CandidateScore,
    pub reason: RejectReason,
    pub candidate_axis: u8,
    pub axis_matches_sequence: bool,
    pub seq_finish_assist: bool,
}

pub fn accel_l1_jerk_and_axis(
    prev: Option<(i16, i16, i16)>,
    current: (i16, i16, i16),
) -> (i32, u8) {
    let Some((px, py, pz)) = prev else {
        return (0, 0);
    };

    let dx = (current.0 as i32 - px as i32).abs();
    let dy = (current.1 as i32 - py as i32).abs();
    let dz = (current.2 as i32 - pz as i32).abs();
    let total = dx + dy + dz;

    let axis = if dx >= dy && dx >= dz {
        LSM6_TAP_SRC_X_BIT
    } else if dy >= dx && dy >= dz {
        LSM6_TAP_SRC_Y_BIT
    } else {
        LSM6_TAP_SRC_Z_BIT
    };

    (total, axis)
}

pub fn compute_motion_features(
    frame: SensorFrame,
    prev_accel: Option<(i16, i16, i16)>,
    prev_jerk_l1: i32,
    last_big_gyro_at_ms: Option<u64>,
    cfg: &TripleTapConfig,
) -> (MotionFeatures, Option<u64>) {
    let gyro_l1 = i32::from(frame.gx).abs() + i32::from(frame.gy).abs() + i32::from(frame.gz).abs();
    let new_last_big_gyro = if gyro_l1 >= cfg.thresholds.gyro_l1_swing_max {
        Some(frame.now_ms)
    } else {
        last_big_gyro_at_ms
    };

    let gyro_veto_active = new_last_big_gyro
        .is_some_and(|last| frame.now_ms.saturating_sub(last) < cfg.gyro_veto_hold_ms);

    let tap_axis_mask =
        frame.tap_src & (LSM6_TAP_SRC_X_BIT | LSM6_TAP_SRC_Y_BIT | LSM6_TAP_SRC_Z_BIT);
    let has_axis_tap = tap_axis_mask != 0;
    let has_single_tap = (frame.tap_src & LSM6_TAP_SRC_SINGLE_TAP_BIT) != 0;
    let has_tap_event = (frame.tap_src & LSM6_TAP_SRC_TAP_EVENT_BIT) != 0;

    let (jerk_l1, jerk_axis) = accel_l1_jerk_and_axis(prev_accel, (frame.ax, frame.ay, frame.az));
    let candidate_axis = if has_axis_tap {
        tap_axis_mask
    } else {
        jerk_axis
    };

    (
        MotionFeatures {
            tap_src: frame.tap_src,
            int1: frame.int1,
            tap_axis_mask,
            has_axis_tap,
            has_single_tap,
            has_tap_event,
            jerk_l1,
            prev_jerk_l1,
            jerk_axis,
            candidate_axis,
            gyro_l1,
            gyro_veto_active,
        },
        new_last_big_gyro,
    )
}

pub fn assess_tap_candidate(
    features: &MotionFeatures,
    seq_count: u8,
    seq_axis: u8,
    last_candidate_at_ms: Option<u64>,
    now_ms: u64,
    cfg: &TripleTapConfig,
) -> CandidateAssessment {
    let axis_matches_sequence =
        seq_axis == 0 || features.candidate_axis == 0 || (seq_axis & features.candidate_axis) != 0;

    let moderate_jerk = features.jerk_l1 >= cfg.thresholds.jerk_l1_min;
    let strong_jerk = features.jerk_l1 >= cfg.thresholds.jerk_strong_l1_min;

    let src_axis = features.has_axis_tap;
    let src_single = features.has_single_tap;
    let src_int1 = features.int1;
    let src_tap_event = features.has_tap_event;
    let src_jerk_axis = features.has_axis_tap && moderate_jerk;
    let src_jerk_only = !features.has_axis_tap
        && strong_jerk
        && features.prev_jerk_l1 <= cfg.thresholds.prev_jerk_quiet_max;
    let src_seq_finish_assist = seq_count >= 2
        && axis_matches_sequence
        && features.jerk_l1 >= cfg.thresholds.jerk_seq_cont_min;

    let fused_tap_candidate = src_jerk_only
        || src_seq_finish_assist
        || (src_axis && (src_single || src_int1 || src_tap_event || src_jerk_axis));

    let mut source_mask = 0u8;
    if src_axis {
        source_mask |= CAND_SRC_AXIS;
    }
    if src_single {
        source_mask |= CAND_SRC_SINGLE;
    }
    if src_int1 {
        source_mask |= CAND_SRC_INT1;
    }
    if src_tap_event {
        source_mask |= CAND_SRC_TAP_EVENT;
    }
    if src_jerk_axis {
        source_mask |= CAND_SRC_JERK_AXIS;
    }
    if src_jerk_only {
        source_mask |= CAND_SRC_JERK_ONLY;
    }
    if src_seq_finish_assist {
        source_mask |= CAND_SRC_SEQ_ASSIST;
    }

    let mut score = 0u16;
    if src_axis {
        score = score.saturating_add(cfg.weights.axis_weight);
    }
    if src_single {
        score = score.saturating_add(cfg.weights.single_tap_weight);
    }
    if src_int1 {
        score = score.saturating_add(cfg.weights.int1_weight);
    }
    if src_tap_event {
        score = score.saturating_add(cfg.weights.tap_event_weight);
    }
    if src_jerk_axis {
        score = score.saturating_add(cfg.weights.jerk_axis_weight);
    }
    if src_jerk_only {
        score = score.saturating_add(cfg.weights.jerk_only_weight);
    }
    if src_seq_finish_assist {
        score = score.saturating_add(cfg.weights.seq_finish_weight);
    }

    if !fused_tap_candidate {
        return CandidateAssessment {
            accepted: false,
            source_mask,
            score: CandidateScore(score),
            reason: RejectReason::CandidateWeak,
            candidate_axis: features.candidate_axis,
            axis_matches_sequence,
            seq_finish_assist: src_seq_finish_assist,
        };
    }

    let debounce_window_ms = if src_seq_finish_assist {
        cfg.seq_finish_debounce_ms
    } else {
        cfg.debounce_ms
    };

    let debounced =
        last_candidate_at_ms.is_some_and(|last| now_ms.saturating_sub(last) < debounce_window_ms);
    if debounced {
        return CandidateAssessment {
            accepted: false,
            source_mask,
            score: CandidateScore(score),
            reason: RejectReason::Debounced,
            candidate_axis: features.candidate_axis,
            axis_matches_sequence,
            seq_finish_assist: src_seq_finish_assist,
        };
    }

    let motion_only_candidate = src_jerk_only || src_seq_finish_assist;
    let veto_candidate = features.gyro_veto_active && motion_only_candidate;
    if veto_candidate {
        source_mask |= CAND_SRC_GYRO_VETO;
        return CandidateAssessment {
            accepted: false,
            source_mask,
            score: CandidateScore(score),
            reason: RejectReason::GyroVeto,
            candidate_axis: features.candidate_axis,
            axis_matches_sequence,
            seq_finish_assist: src_seq_finish_assist,
        };
    }

    CandidateAssessment {
        accepted: true,
        source_mask,
        score: CandidateScore(score),
        reason: RejectReason::None,
        candidate_axis: features.candidate_axis,
        axis_matches_sequence,
        seq_finish_assist: src_seq_finish_assist,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_engine::config::{TapThresholdConfig, TapWeightConfig, TripleTapConfig};

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
            ..frame
        };
        let (features, _) = compute_motion_features(later, None, 0, last_big_gyro, &config);
        assert!(features.gyro_veto_active);

        let much_later = SensorFrame {
            now_ms: 1_250,
            ..frame
        };
        let (features, _) = compute_motion_features(much_later, None, 0, last_big_gyro, &config);
        assert!(!features.gyro_veto_active);
    }

    #[test]
    fn strong_jerk_candidate_is_rejected_when_debounced() {
        let config = cfg();
        let features = MotionFeatures {
            jerk_l1: 3_000,
            prev_jerk_l1: 100,
            candidate_axis: LSM6_TAP_SRC_X_BIT,
            ..MotionFeatures::default()
        };

        let decision = assess_tap_candidate(&features, 0, 0, Some(1_000), 1_050, &config);
        assert!(!decision.accepted);
        assert_eq!(decision.reason, RejectReason::Debounced);
    }
}
