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

mod helpers;
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
mod tests;
