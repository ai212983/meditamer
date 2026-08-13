use embassy_time::{Duration, Instant};

use crate::firmware::{
    config::TAP_TRACE_SAMPLES,
    event_engine::{
        config::active_config,
        types::{EngineAction, EngineStateId, RejectReason},
        EngineTraceSample, EventEngine,
    },
    types::TapTraceSample,
};

use super::super::{
    config::{
        IMU_PIPELINE_INPUTS, IMU_SAMPLING_DEMAND, IMU_TRACE_CONTEXT, TAP_TRACE_ENABLED,
        TAP_TRACE_SAMPLE_MS,
    },
    mailbox::publish_backlight_trigger,
    types::{ImuPipelineInput, ImuSamplingDemand, ImuTraceContext},
};

#[embassy_executor::task]
pub(crate) async fn imu_pipeline_task() {
    let mut event_engine = EventEngine::default();
    let mut trace_context = ImuTraceContext::default();
    let mut next_trace_at = Instant::now();

    loop {
        let input = IMU_PIPELINE_INPUTS.receive().await;
        if let Some(context) = IMU_TRACE_CONTEXT.try_take() {
            trace_context = context;
        }

        match input {
            ImuPipelineInput::Recovered { now_ms } => {
                let _ = event_engine.imu_recovered(now_ms);
                signal_active_until(now_ms);
            }
            ImuPipelineInput::Fault { now_ms, stage } => {
                let _ = stage;
                let _ = event_engine.imu_fault(now_ms);
            }
            ImuPipelineInput::Suppressed { now_ms, reason } => {
                let _ = (now_ms, reason);
                let _ = event_engine.sampling_gap();
            }
            ImuPipelineInput::Resumed { now_ms } => {
                let _ = event_engine.sampling_gap();
                signal_active_until(now_ms);
            }
            ImuPipelineInput::Sample {
                frame,
                int2,
                power_good,
                discontinuity,
            } => {
                if discontinuity {
                    let _ = event_engine.sampling_gap();
                }
                let output = event_engine.tick(frame);
                let trace = output.trace;

                for action in output.actions.iter() {
                    if matches!(action, EngineAction::BacklightTrigger) {
                        publish_backlight_trigger();
                    }
                }

                if should_use_active_cadence(frame.tap_src, frame.int1, trace) {
                    signal_active_until(frame.now_ms);
                }

                let now = Instant::now();
                // The 25 ms floor drops ~2/3 of frames at the 125 Hz active
                // cadence, which is exactly where a tap decision lands. Always
                // emit a frame that carries a tap, a candidate, a live sequence,
                // or a rejection; rate-limit only quiet frames.
                let decisive = frame.tap_src != 0
                    || trace.tap_candidate != 0
                    || trace.seq_count != 0
                    || trace.reject_reason != RejectReason::None;
                if TAP_TRACE_ENABLED && (decisive || now >= next_trace_at) {
                    let sample = TapTraceSample {
                        t_ms: frame.now_ms,
                        tap_src: frame.tap_src,
                        seq_count: trace.seq_count,
                        tap_candidate: trace.tap_candidate,
                        cand_src: trace.candidate_source_mask,
                        state_id: trace.state_id.as_u8(),
                        reject_reason: trace.reject_reason.as_u8(),
                        candidate_score: trace.candidate_score.0,
                        window_ms: trace.window_ms,
                        cooldown_active: trace.cooldown_active,
                        jerk_l1: trace.jerk_l1,
                        motion_veto: trace.motion_veto,
                        gyro_l1: trace.gyro_l1,
                        int1: u8::from(frame.int1),
                        int2: u8::from(int2),
                        power_good,
                        battery_percent: trace_context.battery_percent,
                        gx: frame.gx,
                        gy: frame.gy,
                        gz: frame.gz,
                        ax: frame.ax,
                        ay: frame.ay,
                        az: frame.az,
                    };
                    let _ = TAP_TRACE_SAMPLES.try_send(sample);
                    next_trace_at = now + Duration::from_millis(TAP_TRACE_SAMPLE_MS);
                }
            }
        }
    }
}

fn signal_active_until(now_ms: u64) {
    IMU_SAMPLING_DEMAND.signal(ImuSamplingDemand {
        active_until_ms: now_ms.saturating_add(active_config().imu_sampling.active_hold_ms),
    });
}

fn should_use_active_cadence(tap_src: u8, int1: bool, trace: EngineTraceSample) -> bool {
    let thresholds = &active_config().triple_tap.thresholds;
    tap_src != 0
        || int1
        || trace.tap_candidate != 0
        || matches!(
            trace.state_id,
            EngineStateId::TapSeq1 | EngineStateId::TapSeq2
        )
        || trace.jerk_l1 >= thresholds.jerk_seq_cont_min
        || trace.gyro_l1 >= thresholds.gyro_l1_swing_max / 2
}
