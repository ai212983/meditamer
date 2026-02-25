mod app_events;
mod imu;
mod sd_power;
mod touch_loop;
mod wait;

use super::super::event_engine::{EngineTraceSample, EventEngine};
use app_events::handle_app_event;
use embassy_time::{with_timeout, Duration, Instant};
use imu::process_imu_cycle;
use sd_power::process_sd_power_requests;
use touch_loop::process_touch_cycle;
use wait::{next_loop_wait_ms, LoopWaitSchedule};

use super::super::{
    config::{APP_EVENTS, TAP_TRACE_ENABLED},
    touch::{
        config::{TOUCH_CALIBRATION_WIZARD_ENABLED, TOUCH_INIT_RETRY_MS},
        tasks::{request_touch_pipeline_reset, try_touch_init_with_logs},
        wizard::{render_touch_wizard_waiting_screen, TouchCalibrationWizard},
    },
    types::{DisplayContext, DisplayMode, TimeSyncState},
};
use super::{run_backlight_timeline, FaceDownToggleState};

const SD_POWER_POLL_SLICE_MS: u64 = 5;

#[embassy_executor::task]
pub(crate) async fn display_task(mut context: DisplayContext) {
    let mut update_count = 0u32;
    let mut last_uptime_seconds = 0u32;
    let mut time_sync: Option<TimeSyncState> = None;
    let mut battery_percent: Option<u8> = None;
    let mut display_mode = context
        .mode_store
        .load_mode()
        .unwrap_or(DisplayMode::Shanshui);
    let mut screen_initialized = false;
    let mut pattern_nonce = 0u32;
    let mut first_visual_seed_pending = true;
    let mut face_down_toggle = FaceDownToggleState::new();
    let mut imu_double_tap_ready = false;
    let mut imu_retry_at = Instant::now();
    let mut event_engine = EventEngine::default();
    let mut last_engine_trace = EngineTraceSample::default();
    let mut last_detect_tap_src = 0u8;
    let mut last_detect_int1 = 0u8;
    let trace_epoch = Instant::now();
    let mut tap_trace_next_sample_at = Instant::now();
    let mut tap_trace_aux_next_sample_at = Instant::now();
    let mut tap_trace_power_good: i16 = -1;
    let mut backlight_cycle_start: Option<Instant> = None;
    let mut backlight_level = 0u8;
    let mut touch_ready = try_touch_init_with_logs(&mut context.inkplate, "boot");
    let mut touch_wizard_requested = TOUCH_CALIBRATION_WIZARD_ENABLED;
    let mut touch_wizard = TouchCalibrationWizard::new(touch_wizard_requested && touch_ready);
    let mut touch_retry_at = Instant::now();
    let mut touch_next_sample_at = Instant::now();
    let mut touch_feedback_dirty = false;
    let mut touch_feedback_next_flush_at = Instant::now();
    let mut touch_contact_active = false;
    let mut touch_last_nonzero_at: Option<Instant> = None;
    let mut touch_irq_pending = 0u8;
    let mut touch_irq_burst_until = Instant::now();
    let mut touch_idle_fallback_at = Instant::now();
    let mut touch_wizard_trace_capture_until_ms = 0u64;

    if !touch_ready {
        touch_retry_at = Instant::now() + Duration::from_millis(TOUCH_INIT_RETRY_MS);
    }

    if touch_wizard.is_active() {
        touch_wizard.render_full(&mut context.inkplate).await;
        screen_initialized = true;
    } else if touch_wizard_requested {
        render_touch_wizard_waiting_screen(&mut context.inkplate).await;
        screen_initialized = true;
    }
    request_touch_pipeline_reset();

    loop {
        let app_wait_ms = next_loop_wait_ms(LoopWaitSchedule {
            touch_ready,
            touch_retry_at,
            touch_next_sample_at,
            imu_ready: imu_double_tap_ready,
            imu_retry_at,
            touch_feedback_dirty,
            touch_feedback_next_flush_at,
            tap_trace_active: TAP_TRACE_ENABLED && imu_double_tap_ready,
            tap_trace_next_sample_at,
            tap_trace_aux_next_sample_at,
        });

        let mut event = None;
        let mut remaining_wait_ms = app_wait_ms;
        loop {
            process_sd_power_requests(&mut context).await;

            if remaining_wait_ms == 0 {
                break;
            }
            let wait_slice_ms = remaining_wait_ms.min(SD_POWER_POLL_SLICE_MS);
            match with_timeout(Duration::from_millis(wait_slice_ms), APP_EVENTS.receive()).await {
                Ok(received_event) => {
                    event = Some(received_event);
                    break;
                }
                Err(_) => {
                    remaining_wait_ms = remaining_wait_ms.saturating_sub(wait_slice_ms);
                }
            }
        }

        if let Some(event) = event {
            handle_app_event(
                event,
                &mut context,
                &mut update_count,
                &mut last_uptime_seconds,
                &mut time_sync,
                &mut battery_percent,
                &mut display_mode,
                &mut screen_initialized,
                &mut pattern_nonce,
                &mut first_visual_seed_pending,
                &mut touch_wizard_requested,
                touch_ready,
                &mut touch_wizard,
                &mut touch_last_nonzero_at,
                &mut touch_irq_pending,
                &mut touch_irq_burst_until,
                &mut touch_idle_fallback_at,
                &mut backlight_cycle_start,
                &mut backlight_level,
                &mut touch_next_sample_at,
            )
            .await;
        }

        process_imu_cycle(
            &mut context,
            touch_contact_active,
            touch_last_nonzero_at,
            &mut imu_double_tap_ready,
            &mut imu_retry_at,
            &mut event_engine,
            &mut last_engine_trace,
            &mut last_detect_tap_src,
            &mut last_detect_int1,
            trace_epoch,
            touch_wizard_requested,
            &mut backlight_cycle_start,
            &mut backlight_level,
            &mut face_down_toggle,
            &mut display_mode,
            &mut update_count,
            last_uptime_seconds,
            time_sync,
            battery_percent,
            &mut pattern_nonce,
            &mut first_visual_seed_pending,
            &mut screen_initialized,
            &mut tap_trace_next_sample_at,
            &mut tap_trace_aux_next_sample_at,
            &mut tap_trace_power_good,
        )
        .await;

        process_touch_cycle(
            &mut context,
            &mut touch_ready,
            &mut touch_retry_at,
            &mut touch_next_sample_at,
            &mut touch_contact_active,
            &mut touch_last_nonzero_at,
            &mut touch_irq_pending,
            &mut touch_irq_burst_until,
            &mut touch_idle_fallback_at,
            &mut touch_wizard_trace_capture_until_ms,
            &mut touch_wizard_requested,
            &mut touch_wizard,
            &mut screen_initialized,
            &mut touch_feedback_dirty,
            &mut touch_feedback_next_flush_at,
            &mut backlight_cycle_start,
            &mut backlight_level,
            &mut update_count,
            &mut display_mode,
            last_uptime_seconds,
            time_sync,
            battery_percent,
            &mut pattern_nonce,
            &mut first_visual_seed_pending,
            trace_epoch,
        )
        .await;

        if !touch_wizard_requested {
            run_backlight_timeline(
                &mut context.inkplate,
                &mut backlight_cycle_start,
                &mut backlight_level,
            );
        }
    }
}
