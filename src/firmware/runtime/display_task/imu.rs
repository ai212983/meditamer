use embassy_time::{Duration, Instant};

use super::super::super::{
    app_state::AppStateCommand,
    config::{
        IMU_INIT_RETRY_MS, TAP_TRACE_AUX_SAMPLE_MS, TAP_TRACE_ENABLED, TAP_TRACE_SAMPLES,
        TAP_TRACE_SAMPLE_MS,
    },
    event_engine::SensorFrame,
    render::render_active_mode,
    touch::config::TOUCH_IMU_QUIET_WINDOW_MS,
    types::{DisplayContext, TapTraceSample},
};
use super::super::{trigger_backlight_cycle, update_face_down_toggle};

use super::state::DisplayLoopState;

pub(super) async fn process_imu_cycle(context: &mut DisplayContext, state: &mut DisplayLoopState) {
    let touch_bus_quiet = state.touch_contact_active
        || state.touch_last_nonzero_at.is_some_and(|last_nonzero_at| {
            Instant::now()
                .saturating_duration_since(last_nonzero_at)
                .as_millis()
                <= TOUCH_IMU_QUIET_WINDOW_MS
        });

    if !touch_bus_quiet && !state.imu_double_tap_ready && Instant::now() >= state.imu_retry_at {
        state.imu_double_tap_ready = context.inkplate.lsm6ds3_init_double_tap().unwrap_or(false);
        if state.imu_double_tap_ready {
            let now_ms = Instant::now()
                .saturating_duration_since(state.trace_epoch)
                .as_millis();
            state.last_engine_trace = state.event_engine.imu_recovered(now_ms).trace;
        }
        state.imu_retry_at = Instant::now() + Duration::from_millis(IMU_INIT_RETRY_MS);
    }

    if !touch_bus_quiet && state.imu_double_tap_ready {
        match (
            context.inkplate.lsm6ds3_read_tap_src(),
            context.inkplate.lsm6ds3_int1_level(),
            context.inkplate.lsm6ds3_read_motion_raw(),
        ) {
            (Ok(tap_src), Ok(int1), Ok((gx, gy, gz, ax, ay, az))) => {
                let now = Instant::now();
                let now_ms = now.saturating_duration_since(state.trace_epoch).as_millis();
                state.last_detect_tap_src = tap_src;
                state.last_detect_int1 = if int1 { 1 } else { 0 };

                let output = state.event_engine.tick(SensorFrame {
                    now_ms,
                    tap_src,
                    int1,
                    gx,
                    gy,
                    gz,
                    ax,
                    ay,
                    az,
                });
                state.last_engine_trace = output.trace;

                if output.actions.contains_backlight_trigger() && !state.in_touch_wizard_mode() {
                    trigger_backlight_cycle(
                        &mut context.inkplate,
                        &mut state.backlight_cycle_start,
                        &mut state.backlight_level,
                    );
                }

                if update_face_down_toggle(&mut state.face_down_toggle, now, ax, ay, az) {
                    let _ = state
                        .apply_state_command(context, AppStateCommand::ToggleDayBackground)
                        .await;
                    state.update_count = 0;
                    let last_uptime_seconds = state.last_uptime_seconds;
                    let time_sync = state.time_sync;
                    let battery_percent = state.battery_percent;
                    render_active_mode(
                        &mut context.inkplate,
                        state.base_mode(),
                        state.day_background(),
                        state.overlay_mode(),
                        (last_uptime_seconds, time_sync, battery_percent),
                        (
                            &mut state.pattern_nonce,
                            &mut state.first_visual_seed_pending,
                        ),
                    )
                    .await;
                    state.screen_initialized = true;
                }
            }
            _ => {
                state.imu_double_tap_ready = false;
                let now_ms = Instant::now()
                    .saturating_duration_since(state.trace_epoch)
                    .as_millis();
                state.last_engine_trace = state.event_engine.imu_fault(now_ms).trace;
                state.last_detect_tap_src = 0;
                state.last_detect_int1 = 0;
                state.imu_retry_at = Instant::now() + Duration::from_millis(IMU_INIT_RETRY_MS);
            }
        }
    }

    if TAP_TRACE_ENABLED && state.imu_double_tap_ready && !touch_bus_quiet {
        let now = Instant::now();

        if now >= state.tap_trace_aux_next_sample_at {
            state.tap_trace_power_good = context
                .inkplate
                .read_power_good()
                .ok()
                .map(|v| v as i16)
                .unwrap_or(-1);
            state.tap_trace_aux_next_sample_at =
                now + Duration::from_millis(TAP_TRACE_AUX_SAMPLE_MS);
        }

        if now >= state.tap_trace_next_sample_at {
            if let (Ok(int2), Ok((gx, gy, gz, ax, ay, az))) = (
                context.inkplate.lsm6ds3_int2_level(),
                context.inkplate.lsm6ds3_read_motion_raw(),
            ) {
                let battery_percent_i16 = state.battery_percent.map_or(-1, i16::from);
                let t_ms = now.saturating_duration_since(state.trace_epoch).as_millis();
                let sample = TapTraceSample {
                    t_ms,
                    tap_src: state.last_detect_tap_src,
                    seq_count: state.last_engine_trace.seq_count,
                    tap_candidate: state.last_engine_trace.tap_candidate,
                    cand_src: state.last_engine_trace.candidate_source_mask,
                    state_id: state.last_engine_trace.state_id.as_u8(),
                    reject_reason: state.last_engine_trace.reject_reason.as_u8(),
                    candidate_score: state.last_engine_trace.candidate_score.0,
                    window_ms: state.last_engine_trace.window_ms,
                    cooldown_active: state.last_engine_trace.cooldown_active,
                    jerk_l1: state.last_engine_trace.jerk_l1,
                    motion_veto: state.last_engine_trace.motion_veto,
                    gyro_l1: state.last_engine_trace.gyro_l1,
                    int1: state.last_detect_int1,
                    int2: if int2 { 1 } else { 0 },
                    power_good: state.tap_trace_power_good,
                    battery_percent: battery_percent_i16,
                    gx,
                    gy,
                    gz,
                    ax,
                    ay,
                    az,
                };
                let _ = TAP_TRACE_SAMPLES.try_send(sample);
            }
            state.tap_trace_next_sample_at = now + Duration::from_millis(TAP_TRACE_SAMPLE_MS);
        }
    }
}
