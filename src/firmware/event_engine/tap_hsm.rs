use core::cmp::min;

use statig::{blocking::IntoStateMachineExt as _, prelude::*};

use super::{
    config::{active_config, EventEngineConfig},
    features::{assess_tap_candidate, compute_motion_features, CandidateAssessment},
    trace::EngineTraceSample,
    types::{
        ActionBuffer, CandidateScore, EngineAction, EngineStateId, EventDetected, EventKind,
        MotionFeatures, RejectReason, SensorFrame,
    },
};

#[derive(Clone, Copy, Debug)]
enum TapHsmEvent {
    Tick(SensorFrame),
    ImuFault { now_ms: u64 },
    ImuRecovered { now_ms: u64 },
}

#[derive(Default)]
struct DispatchContext {
    actions: ActionBuffer,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EngineOutput {
    pub actions: ActionBuffer,
    pub trace: EngineTraceSample,
}

pub struct EventEngine {
    machine: statig::blocking::StateMachine<TapHsm>,
}

impl Default for EventEngine {
    fn default() -> Self {
        Self::new(active_config())
    }
}

impl EventEngine {
    pub fn new(config: &'static EventEngineConfig) -> Self {
        Self {
            machine: TapHsm::new(config).state_machine(),
        }
    }

    pub fn tick(&mut self, frame: SensorFrame) -> EngineOutput {
        let mut context = DispatchContext::default();
        self.machine
            .handle_with_context(&TapHsmEvent::Tick(frame), &mut context);
        self.finish(context)
    }

    pub fn imu_fault(&mut self, now_ms: u64) -> EngineOutput {
        let mut context = DispatchContext::default();
        self.machine
            .handle_with_context(&TapHsmEvent::ImuFault { now_ms }, &mut context);
        self.finish(context)
    }

    pub fn imu_recovered(&mut self, now_ms: u64) -> EngineOutput {
        let mut context = DispatchContext::default();
        self.machine
            .handle_with_context(&TapHsmEvent::ImuRecovered { now_ms }, &mut context);
        self.finish(context)
    }

    fn finish(&self, context: DispatchContext) -> EngineOutput {
        EngineOutput {
            actions: context.actions,
            trace: self.machine.inner().last_trace,
        }
    }
}

struct TapHsm {
    config: &'static EventEngineConfig,
    prev_accel: Option<(i16, i16, i16)>,
    prev_jerk_l1: i32,
    last_candidate_at_ms: Option<u64>,
    last_big_gyro_at_ms: Option<u64>,
    seq_last_tap_ms: Option<u64>,
    seq_axis: u8,
    last_trigger_at_ms: Option<u64>,
    last_trace: EngineTraceSample,
}

impl TapHsm {
    fn new(config: &'static EventEngineConfig) -> Self {
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

    fn in_cooldown(&self, now_ms: u64) -> bool {
        self.last_trigger_at_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < self.config.triple_tap.cooldown_ms)
    }

    fn window_ms(&self, now_ms: u64) -> u16 {
        let dt = self
            .seq_last_tap_ms
            .map_or(0, |last| now_ms.saturating_sub(last));
        min(dt, u16::MAX as u64) as u16
    }

    fn clear_sequence(&mut self) {
        self.seq_last_tap_ms = None;
        self.seq_axis = 0;
    }

    fn start_sequence(&mut self, now_ms: u64, axis: u8) {
        self.seq_last_tap_ms = Some(now_ms);
        self.seq_axis = axis;
    }

    fn confidence_from_score(score: CandidateScore) -> u8 {
        min(score.0, 100) as u8
    }

    fn reject_with_reason(&mut self, reason: RejectReason) {
        self.last_trace.reject_reason = reason;
    }

    fn push_counter_reset(context: &mut DispatchContext, reason: RejectReason) {
        context.actions.push(EngineAction::CounterReset { reason });
    }

    fn push_trigger_actions(context: &mut DispatchContext, score: CandidateScore, source_mask: u8) {
        context.actions.push(EngineAction::BacklightTrigger);
        context
            .actions
            .push(EngineAction::EventDetected(EventDetected {
                kind: EventKind::DoubleTap,
                confidence: Self::confidence_from_score(score),
                source_mask,
            }));
    }

    fn update_tick_trace(
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

    fn update_fault_trace(&mut self, state_id: EngineStateId, now_ms: u64) {
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

    fn evaluate_tick(
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

#[state_machine(initial = "State::idle()")]
impl TapHsm {
    #[state(superstate = "active")]
    fn idle(&mut self, context: &mut DispatchContext, event: &TapHsmEvent) -> Outcome<State> {
        let _ = context;
        match event {
            TapHsmEvent::Tick(frame) => {
                let (_, assessment) = self.evaluate_tick(EngineStateId::Idle, 0, *frame);
                if assessment.accepted {
                    if self.in_cooldown(frame.now_ms) {
                        self.reject_with_reason(RejectReason::CooldownActive);
                        return Transition(State::triggered_cooldown());
                    }
                    self.start_sequence(frame.now_ms, assessment.candidate_axis);
                    return Transition(State::tap_seq1());
                }
                Handled
            }
            TapHsmEvent::ImuRecovered { now_ms } => {
                self.update_fault_trace(EngineStateId::Idle, *now_ms);
                Handled
            }
            TapHsmEvent::ImuFault { .. } => Super,
        }
    }

    #[state(superstate = "active")]
    fn tap_seq1(&mut self, context: &mut DispatchContext, event: &TapHsmEvent) -> Outcome<State> {
        match event {
            TapHsmEvent::Tick(frame) => {
                let (_, assessment) = self.evaluate_tick(EngineStateId::TapSeq1, 1, *frame);

                let Some(last_tap_ms) = self.seq_last_tap_ms else {
                    self.clear_sequence();
                    return Transition(State::idle());
                };

                let dt = frame.now_ms.saturating_sub(last_tap_ms);
                if dt > self.config.triple_tap.max_gap_ms {
                    self.clear_sequence();
                    self.reject_with_reason(RejectReason::GapTooLong);
                    Self::push_counter_reset(context, RejectReason::GapTooLong);
                    return Transition(State::idle());
                }

                if !assessment.accepted {
                    return Handled;
                }

                if !assessment.axis_matches_sequence {
                    self.start_sequence(frame.now_ms, assessment.candidate_axis);
                    self.reject_with_reason(RejectReason::AxisMismatch);
                    Self::push_counter_reset(context, RejectReason::AxisMismatch);
                    return Handled;
                }

                if dt < self.config.triple_tap.min_gap_ms {
                    self.start_sequence(frame.now_ms, assessment.candidate_axis);
                    self.reject_with_reason(RejectReason::GapTooShort);
                    Self::push_counter_reset(context, RejectReason::GapTooShort);
                    return Handled;
                }

                self.start_sequence(frame.now_ms, assessment.candidate_axis);
                Transition(State::tap_seq2())
            }
            TapHsmEvent::ImuRecovered { now_ms } => {
                self.update_fault_trace(EngineStateId::TapSeq1, *now_ms);
                Handled
            }
            TapHsmEvent::ImuFault { .. } => Super,
        }
    }

    #[state(superstate = "active")]
    fn tap_seq2(&mut self, context: &mut DispatchContext, event: &TapHsmEvent) -> Outcome<State> {
        match event {
            TapHsmEvent::Tick(frame) => {
                let (_, assessment) = self.evaluate_tick(EngineStateId::TapSeq2, 2, *frame);

                let Some(last_tap_ms) = self.seq_last_tap_ms else {
                    self.clear_sequence();
                    return Transition(State::idle());
                };

                let dt = frame.now_ms.saturating_sub(last_tap_ms);
                if dt > self.config.triple_tap.last_max_gap_ms {
                    self.clear_sequence();
                    self.reject_with_reason(RejectReason::GapTooLong);
                    Self::push_counter_reset(context, RejectReason::GapTooLong);
                    return Transition(State::idle());
                }

                if !assessment.accepted {
                    return Handled;
                }

                if !assessment.axis_matches_sequence {
                    self.start_sequence(frame.now_ms, assessment.candidate_axis);
                    self.reject_with_reason(RejectReason::AxisMismatch);
                    Self::push_counter_reset(context, RejectReason::AxisMismatch);
                    return Transition(State::tap_seq1());
                }

                if dt < self.config.triple_tap.min_gap_ms {
                    self.start_sequence(frame.now_ms, assessment.candidate_axis);
                    self.reject_with_reason(RejectReason::GapTooShort);
                    Self::push_counter_reset(context, RejectReason::GapTooShort);
                    return Transition(State::tap_seq1());
                }

                self.clear_sequence();
                if self.in_cooldown(frame.now_ms) {
                    self.reject_with_reason(RejectReason::CooldownActive);
                    return Transition(State::triggered_cooldown());
                }

                self.last_trigger_at_ms = Some(frame.now_ms);
                Self::push_trigger_actions(context, assessment.score, assessment.source_mask);
                Transition(State::triggered_cooldown())
            }
            TapHsmEvent::ImuRecovered { now_ms } => {
                self.update_fault_trace(EngineStateId::TapSeq2, *now_ms);
                Handled
            }
            TapHsmEvent::ImuFault { .. } => Super,
        }
    }

    #[state(superstate = "active")]
    fn triggered_cooldown(
        &mut self,
        context: &mut DispatchContext,
        event: &TapHsmEvent,
    ) -> Outcome<State> {
        let _ = context;
        match event {
            TapHsmEvent::Tick(frame) => {
                let (_, assessment) =
                    self.evaluate_tick(EngineStateId::TriggeredCooldown, 0, *frame);
                if !self.in_cooldown(frame.now_ms) {
                    return Transition(State::idle());
                }

                if assessment.accepted {
                    self.reject_with_reason(RejectReason::CooldownActive);
                }
                Handled
            }
            TapHsmEvent::ImuRecovered { now_ms } => {
                self.update_fault_trace(EngineStateId::TriggeredCooldown, *now_ms);
                Handled
            }
            TapHsmEvent::ImuFault { .. } => Super,
        }
    }

    #[state(superstate = "suppressed")]
    fn sensor_fault_backoff(
        &mut self,
        context: &mut DispatchContext,
        event: &TapHsmEvent,
    ) -> Outcome<State> {
        let _ = context;
        match event {
            TapHsmEvent::Tick(frame) => {
                self.last_trace = EngineTraceSample {
                    now_ms: frame.now_ms,
                    state_id: EngineStateId::SensorFaultBackoff,
                    reject_reason: RejectReason::SensorFault,
                    seq_count: 0,
                    tap_candidate: 0,
                    candidate_source_mask: 0,
                    candidate_score: CandidateScore(0),
                    window_ms: 0,
                    cooldown_active: 0,
                    tap_src: frame.tap_src,
                    jerk_l1: 0,
                    motion_veto: 0,
                    gyro_l1: i32::from(frame.gx).abs()
                        + i32::from(frame.gy).abs()
                        + i32::from(frame.gz).abs(),
                };
                Handled
            }
            TapHsmEvent::ImuFault { now_ms } => {
                self.update_fault_trace(EngineStateId::SensorFaultBackoff, *now_ms);
                Handled
            }
            TapHsmEvent::ImuRecovered { .. } => Super,
        }
    }

    #[superstate]
    fn active(&mut self, context: &mut DispatchContext, event: &TapHsmEvent) -> Outcome<State> {
        let _ = context;
        match event {
            TapHsmEvent::ImuFault { now_ms } => {
                self.clear_sequence();
                self.update_fault_trace(EngineStateId::SensorFaultBackoff, *now_ms);
                Transition(State::sensor_fault_backoff())
            }
            _ => Super,
        }
    }

    #[superstate]
    fn suppressed(&mut self, context: &mut DispatchContext, event: &TapHsmEvent) -> Outcome<State> {
        let _ = context;
        match event {
            TapHsmEvent::ImuRecovered { now_ms } => {
                self.update_fault_trace(EngineStateId::Idle, *now_ms);
                Transition(State::idle())
            }
            _ => Handled,
        }
    }
}

#[cfg(test)]
mod tests {
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
}
