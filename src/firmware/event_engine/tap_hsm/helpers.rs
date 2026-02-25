use super::*;

impl TapHsm {
    pub(super) fn new(config: &'static EventEngineConfig) -> Self {
        Self {
            config,
            prev_accel: None,
            prev_jerk_l1: 0,
            last_candidate_at_ms: None,
            last_big_gyro_at_ms: None,
            seq_last_tap_ms: None,
            seq_axis: 0,
            last_trigger_at_ms: None,
            last_trace: EngineTraceSample {
                state_id: EngineStateId::Idle,
                ..EngineTraceSample::default()
            },
        }
    }

    pub(super) fn in_cooldown(&self, now_ms: u64) -> bool {
        self.last_trigger_at_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < self.config.triple_tap.cooldown_ms)
    }

    pub(super) fn window_ms(&self, now_ms: u64) -> u16 {
        let dt = self
            .seq_last_tap_ms
            .map_or(0, |last| now_ms.saturating_sub(last));
        min(dt, u16::MAX as u64) as u16
    }

    pub(super) fn clear_sequence(&mut self) {
        self.seq_last_tap_ms = None;
        self.seq_axis = 0;
    }

    pub(super) fn start_sequence(&mut self, now_ms: u64, axis: u8) {
        self.seq_last_tap_ms = Some(now_ms);
        self.seq_axis = axis;
    }

    pub(super) fn confidence_from_score(score: CandidateScore) -> u8 {
        min(score.0, 100) as u8
    }

    pub(super) fn reject_with_reason(&mut self, reason: RejectReason) {
        self.last_trace.reject_reason = reason;
    }

    pub(super) fn push_counter_reset(context: &mut DispatchContext, reason: RejectReason) {
        context.actions.push(EngineAction::CounterReset { reason });
    }

    pub(super) fn push_trigger_actions(
        context: &mut DispatchContext,
        score: CandidateScore,
        source_mask: u8,
    ) {
        context.actions.push(EngineAction::BacklightTrigger);
        context
            .actions
            .push(EngineAction::EventDetected(EventDetected {
                kind: EventKind::DoubleTap,
                confidence: Self::confidence_from_score(score),
                source_mask,
            }));
    }

    pub(super) fn update_tick_trace(
        &mut self,
        state_id: EngineStateId,
        frame: SensorFrame,
        features: MotionFeatures,
        assessment: CandidateAssessment,
        seq_count: u8,
    ) {
        self.last_trace = EngineTraceSample {
            now_ms: frame.now_ms,
            state_id,
            reject_reason: assessment.reason,
            seq_count,
            tap_candidate: if assessment.accepted { 1 } else { 0 },
            candidate_source_mask: assessment.source_mask,
            candidate_score: assessment.score,
            window_ms: self.window_ms(frame.now_ms),
            cooldown_active: if self.in_cooldown(frame.now_ms) { 1 } else { 0 },
            tap_src: frame.tap_src,
            jerk_l1: features.jerk_l1,
            motion_veto: if features.gyro_veto_active { 1 } else { 0 },
            gyro_l1: features.gyro_l1,
        };
    }

    pub(super) fn update_fault_trace(&mut self, state_id: EngineStateId, now_ms: u64) {
        self.last_trace = EngineTraceSample {
            now_ms,
            state_id,
            reject_reason: RejectReason::SensorFault,
            seq_count: 0,
            tap_candidate: 0,
            candidate_source_mask: 0,
            candidate_score: CandidateScore(0),
            window_ms: 0,
            cooldown_active: 0,
            tap_src: 0,
            jerk_l1: 0,
            motion_veto: 0,
            gyro_l1: 0,
        };
    }

    pub(super) fn evaluate_tick(
        &mut self,
        state_id: EngineStateId,
        seq_count: u8,
        frame: SensorFrame,
    ) -> (MotionFeatures, CandidateAssessment) {
        let (features, new_last_big_gyro_at_ms) = compute_motion_features(
            frame,
            self.prev_accel,
            self.prev_jerk_l1,
            self.last_big_gyro_at_ms,
            &self.config.triple_tap,
        );
        self.last_big_gyro_at_ms = new_last_big_gyro_at_ms;

        let assessment = if self.config.triple_tap.enabled {
            assess_tap_candidate(
                &features,
                seq_count,
                self.seq_axis,
                self.last_candidate_at_ms,
                frame.now_ms,
                &self.config.triple_tap,
            )
        } else {
            CandidateAssessment {
                accepted: false,
                source_mask: 0,
                score: CandidateScore(0),
                reason: RejectReason::CandidateWeak,
                candidate_axis: features.candidate_axis,
                axis_matches_sequence: true,
                seq_finish_assist: false,
            }
        };

        if assessment.accepted {
            self.last_candidate_at_ms = Some(frame.now_ms);
        }

        self.update_tick_trace(state_id, frame, features, assessment, seq_count);
        self.prev_accel = Some((frame.ax, frame.ay, frame.az));
        self.prev_jerk_l1 = features.jerk_l1;

        (features, assessment)
    }
}
