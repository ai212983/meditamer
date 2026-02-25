use embassy_time::{Duration, Instant};

use super::super::super::{
    config::{
        IMU_INIT_RETRY_MS, TAP_TRACE_AUX_SAMPLE_MS, TAP_TRACE_ENABLED, TAP_TRACE_SAMPLES,
        TAP_TRACE_SAMPLE_MS,
    },
    event_engine::{EngineTraceSample, EventEngine, SensorFrame},
    render::render_active_mode,
    touch::config::TOUCH_IMU_QUIET_WINDOW_MS,
    types::{DisplayContext, DisplayMode, TapTraceSample, TimeSyncState},
};
use super::super::{trigger_backlight_cycle, update_face_down_toggle, FaceDownToggleState};

#[allow(clippy::too_many_arguments)]
pub(super) async fn process_imu_cycle(
    context: &mut DisplayContext,
    touch_contact_active: bool,
    touch_last_nonzero_at: Option<Instant>,
    imu_double_tap_ready: &mut bool,
    imu_retry_at: &mut Instant,
    event_engine: &mut EventEngine,
    last_engine_trace: &mut EngineTraceSample,
    last_detect_tap_src: &mut u8,
    last_detect_int1: &mut u8,
    trace_epoch: Instant,
    touch_wizard_requested: bool,
    backlight_cycle_start: &mut Option<Instant>,
    backlight_level: &mut u8,
    face_down_toggle: &mut FaceDownToggleState,
    display_mode: &mut DisplayMode,
    update_count: &mut u32,
    last_uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
    battery_percent: Option<u8>,
    pattern_nonce: &mut u32,
    first_visual_seed_pending: &mut bool,
    screen_initialized: &mut bool,
    tap_trace_next_sample_at: &mut Instant,
    tap_trace_aux_next_sample_at: &mut Instant,
    tap_trace_power_good: &mut i16,
) {
    let touch_bus_quiet = touch_contact_active
        || touch_last_nonzero_at.is_some_and(|last_nonzero_at| {
            Instant::now()
                .saturating_duration_since(last_nonzero_at)
                .as_millis()
                <= TOUCH_IMU_QUIET_WINDOW_MS
        });

    if !touch_bus_quiet && !*imu_double_tap_ready && Instant::now() >= *imu_retry_at {
        *imu_double_tap_ready = context.inkplate.lsm6ds3_init_double_tap().unwrap_or(false);
        if *imu_double_tap_ready {
            let now_ms = Instant::now()
                .saturating_duration_since(trace_epoch)
                .as_millis();
            *last_engine_trace = event_engine.imu_recovered(now_ms).trace;
        }
        *imu_retry_at = Instant::now() + Duration::from_millis(IMU_INIT_RETRY_MS);
    }

    if !touch_bus_quiet && *imu_double_tap_ready {
        match (
            context.inkplate.lsm6ds3_read_tap_src(),
            context.inkplate.lsm6ds3_int1_level(),
            context.inkplate.lsm6ds3_read_motion_raw(),
        ) {
            (Ok(tap_src), Ok(int1), Ok((gx, gy, gz, ax, ay, az))) => {
                let now = Instant::now();
                let now_ms = now.saturating_duration_since(trace_epoch).as_millis();
                *last_detect_tap_src = tap_src;
                *last_detect_int1 = if int1 { 1 } else { 0 };

                let output = event_engine.tick(SensorFrame {
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
                *last_engine_trace = output.trace;

                if output.actions.contains_backlight_trigger() && !touch_wizard_requested {
                    trigger_backlight_cycle(
                        &mut context.inkplate,
                        backlight_cycle_start,
                        backlight_level,
                    );
                }

                if update_face_down_toggle(face_down_toggle, now, ax, ay, az) {
                    *display_mode = display_mode.toggled();
                    context.mode_store.save_mode(*display_mode);
                    *update_count = 0;
                    render_active_mode(
                        &mut context.inkplate,
                        *display_mode,
                        last_uptime_seconds,
                        time_sync,
                        battery_percent,
                        (pattern_nonce, first_visual_seed_pending),
                        true,
                    )
                    .await;
                    *screen_initialized = true;
                }
            }
            _ => {
                *imu_double_tap_ready = false;
                let now_ms = Instant::now()
                    .saturating_duration_since(trace_epoch)
                    .as_millis();
                *last_engine_trace = event_engine.imu_fault(now_ms).trace;
                *last_detect_tap_src = 0;
                *last_detect_int1 = 0;
                *imu_retry_at = Instant::now() + Duration::from_millis(IMU_INIT_RETRY_MS);
            }
        }
    }

    if TAP_TRACE_ENABLED && *imu_double_tap_ready && !touch_bus_quiet {
        let now = Instant::now();

        if now >= *tap_trace_aux_next_sample_at {
            *tap_trace_power_good = context
                .inkplate
                .read_power_good()
                .ok()
                .map(|v| v as i16)
                .unwrap_or(-1);
            *tap_trace_aux_next_sample_at = now + Duration::from_millis(TAP_TRACE_AUX_SAMPLE_MS);
        }

        if now >= *tap_trace_next_sample_at {
            if let (Ok(int2), Ok((gx, gy, gz, ax, ay, az))) = (
                context.inkplate.lsm6ds3_int2_level(),
                context.inkplate.lsm6ds3_read_motion_raw(),
            ) {
                let battery_percent_i16 = battery_percent.map_or(-1, i16::from);
                let t_ms = now.saturating_duration_since(trace_epoch).as_millis();
                let sample = TapTraceSample {
                    t_ms,
                    tap_src: *last_detect_tap_src,
                    seq_count: last_engine_trace.seq_count,
                    tap_candidate: last_engine_trace.tap_candidate,
                    cand_src: last_engine_trace.candidate_source_mask,
                    state_id: last_engine_trace.state_id.as_u8(),
                    reject_reason: last_engine_trace.reject_reason.as_u8(),
                    candidate_score: last_engine_trace.candidate_score.0,
                    window_ms: last_engine_trace.window_ms,
                    cooldown_active: last_engine_trace.cooldown_active,
                    jerk_l1: last_engine_trace.jerk_l1,
                    motion_veto: last_engine_trace.motion_veto,
                    gyro_l1: last_engine_trace.gyro_l1,
                    int1: *last_detect_int1,
                    int2: if int2 { 1 } else { 0 },
                    power_good: *tap_trace_power_good,
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
            *tap_trace_next_sample_at = now + Duration::from_millis(TAP_TRACE_SAMPLE_MS);
        }
    }
}
