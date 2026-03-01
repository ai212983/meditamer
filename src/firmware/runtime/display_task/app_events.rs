use core::sync::atomic::Ordering;

use embassy_time::{Duration, Instant};

use super::super::super::{
    app_state::{AppStateCommand, BaseMode, OverlayMode},
    config::{APP_STATE_APPLY_ACKS, FULL_REFRESH_EVERY_N_UPDATES},
    render::{
        next_visual_seed, render_active_mode, render_clock_overlay, render_shanshui_update,
        render_suminagashi_update, render_visual_update, sample_battery_percent,
        RenderActiveParams, RenderVisualParams,
    },
    touch::{
        config::{TOUCH_IRQ_BURST_MS, TOUCH_IRQ_LOW, TOUCH_SAMPLE_IDLE_FALLBACK_MS},
        tasks::request_touch_pipeline_reset,
        wizard::{render_touch_wizard_waiting_screen, TouchCalibrationWizard},
    },
    types::{AppEvent, AppStateApplyAck, DisplayContext, TimeSyncState},
};

use super::state::DisplayLoopState;

fn apply_status_code(status: crate::firmware::app_state::actions::AppStateApplyStatus) -> u8 {
    match status {
        crate::firmware::app_state::actions::AppStateApplyStatus::Applied => 0,
        crate::firmware::app_state::actions::AppStateApplyStatus::Unchanged => 1,
        crate::firmware::app_state::actions::AppStateApplyStatus::InvalidTransition => 2,
    }
}

pub(super) async fn handle_app_event(
    event: AppEvent,
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
) {
    let upload_enabled = state.upload_enabled();
    match event {
        AppEvent::Refresh { uptime_seconds } => {
            handle_refresh_event(uptime_seconds, context, state, upload_enabled).await;
        }
        AppEvent::BatteryTick => {
            handle_battery_tick_event(context, state, upload_enabled).await;
        }
        AppEvent::TimeSync(cmd) => {
            handle_time_sync_event(cmd, context, state, upload_enabled).await;
        }
        AppEvent::TouchIrq => {
            handle_touch_irq_event(context, state, upload_enabled);
        }
        AppEvent::StartTouchCalibrationWizard => {
            handle_start_touch_calibration_wizard_event(context, state, upload_enabled).await;
        }
        AppEvent::ForceRepaint => {
            handle_force_repaint_event(context, state, upload_enabled).await;
        }
        AppEvent::ForceMarbleRepaint => {
            handle_force_marble_repaint_event(context, state, upload_enabled).await;
        }
        AppEvent::ApplyAppStateCommand {
            command,
            ack_request_id,
        } => {
            handle_apply_app_state_command_event(context, state, command, ack_request_id).await;
        }
    }
}

fn should_do_full_refresh(state: &DisplayLoopState) -> bool {
    !state.screen_initialized
        || state
            .update_count
            .is_multiple_of(FULL_REFRESH_EVERY_N_UPDATES)
}

async fn render_active_mode_from_state(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    uptime_seconds: u32,
) {
    render_active_mode(
        &mut context.inkplate,
        RenderActiveParams {
            base_mode: state.base_mode(),
            day_background: state.day_background(),
            overlay_mode: state.overlay_mode(),
            uptime_seconds,
            time_sync: state.time_sync,
            battery_percent: state.battery_percent,
            pattern_nonce: &mut state.pattern_nonce,
            first_visual_seed_pending: &mut state.first_visual_seed_pending,
        },
    )
    .await;
}

async fn render_visual_update_from_state(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    uptime_seconds: u32,
) {
    render_visual_update(
        &mut context.inkplate,
        RenderVisualParams {
            day_background: state.day_background(),
            overlay_mode: state.overlay_mode(),
            uptime_seconds,
            time_sync: state.time_sync,
            battery_percent: state.battery_percent,
            pattern_nonce: &mut state.pattern_nonce,
            first_visual_seed_pending: &mut state.first_visual_seed_pending,
        },
    )
    .await;
}

async fn handle_refresh_event(
    uptime_seconds: u32,
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    upload_enabled: bool,
) {
    state.last_uptime_seconds = uptime_seconds;
    if upload_enabled || state.in_touch_wizard_mode() {
        return;
    }
    if should_do_full_refresh(state) {
        render_active_mode_from_state(context, state, uptime_seconds).await;
    } else {
        render_visual_update_from_state(context, state, uptime_seconds).await;
    }
    state.update_count = state.update_count.wrapping_add(1);
    state.screen_initialized = true;
}

async fn handle_battery_tick_event(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    upload_enabled: bool,
) {
    if upload_enabled {
        return;
    }
    if let Some(sampled_percent) = sample_battery_percent(&mut context.inkplate) {
        state.battery_percent = Some(sampled_percent);
    }
    if state.in_touch_wizard_mode() {
        return;
    }
    if state.screen_initialized {
        if matches!(state.overlay_mode(), OverlayMode::Clock) {
            render_clock_overlay(
                &mut context.inkplate,
                state.last_uptime_seconds,
                state.time_sync,
                state.battery_percent,
            )
            .await;
        }
        return;
    }
    render_active_mode_from_state(context, state, state.last_uptime_seconds).await;
    state.screen_initialized = true;
}

async fn handle_time_sync_event(
    cmd: crate::firmware::types::SetTimeSyncCommand,
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    upload_enabled: bool,
) {
    let uptime_now = Instant::now().as_secs().min(u32::MAX as u64) as u32;
    state.last_uptime_seconds = state.last_uptime_seconds.max(uptime_now);
    state.time_sync = Some(TimeSyncState {
        unix_epoch_utc_seconds: cmd.unix_epoch_utc_seconds,
        tz_offset_minutes: cmd.tz_offset_minutes,
        sync_instant: Instant::now(),
    });
    state.update_count = 0;
    if upload_enabled || state.in_touch_wizard_mode() {
        return;
    }
    render_active_mode_from_state(context, state, state.last_uptime_seconds).await;
    state.screen_initialized = true;
}

fn handle_touch_irq_event(
    _context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    upload_enabled: bool,
) {
    if upload_enabled {
        return;
    }
    state.touch_irq_pending = state.touch_irq_pending.saturating_add(1);
    let now = Instant::now();
    state.touch_irq_burst_until = now + Duration::from_millis(TOUCH_IRQ_BURST_MS);
    if state.touch_next_sample_at > now {
        state.touch_next_sample_at = now;
    }
    state.touch_idle_fallback_at = now + Duration::from_millis(TOUCH_SAMPLE_IDLE_FALLBACK_MS);
}

async fn handle_start_touch_calibration_wizard_event(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    upload_enabled: bool,
) {
    if upload_enabled {
        return;
    }
    let _ = state
        .apply_state_command(context, AppStateCommand::SetBase(BaseMode::TouchWizard))
        .await;
    esp_println::println!(
        "touch_wizard: start_event touch_ready={}",
        state.touch_ready
    );
    state.touch_last_nonzero_at = None;
    state.touch_irq_pending = 0;
    state.touch_irq_burst_until = Instant::now();
    TOUCH_IRQ_LOW.store(false, Ordering::Relaxed);
    state.touch_idle_fallback_at =
        Instant::now() + Duration::from_millis(TOUCH_SAMPLE_IDLE_FALLBACK_MS);
    state.backlight_cycle_start = None;
    state.backlight_level = 0;
    let _ = context.inkplate.frontlight_off();
    request_touch_pipeline_reset();
    state.touch_next_sample_at = Instant::now();
    setup_touch_wizard_screen(context, state).await;
}

async fn setup_touch_wizard_screen(context: &mut DisplayContext, state: &mut DisplayLoopState) {
    if state.touch_ready {
        state.touch_wizard = TouchCalibrationWizard::new(true);
        state.touch_wizard.render_full(&mut context.inkplate).await;
    } else {
        state.touch_wizard = TouchCalibrationWizard::new(false);
        render_touch_wizard_waiting_screen(&mut context.inkplate).await;
    }
    state.screen_initialized = true;
}

async fn handle_force_repaint_event(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    upload_enabled: bool,
) {
    if upload_enabled || state.in_touch_wizard_mode() {
        return;
    }
    state.update_count = 0;
    render_active_mode_from_state(context, state, state.last_uptime_seconds).await;
    state.screen_initialized = true;
}

async fn handle_force_marble_repaint_event(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    upload_enabled: bool,
) {
    if upload_enabled || state.in_touch_wizard_mode() {
        return;
    }
    let last_uptime_seconds = state.last_uptime_seconds;
    let time_sync = state.time_sync;
    let seed = next_visual_seed(
        last_uptime_seconds,
        time_sync,
        &mut state.pattern_nonce,
        &mut state.first_visual_seed_pending,
    );
    if matches!(
        state.day_background(),
        crate::firmware::app_state::DayBackground::Shanshui
    ) {
        render_shanshui_update(&mut context.inkplate, seed, last_uptime_seconds, time_sync).await;
    } else {
        render_suminagashi_update(&mut context.inkplate, seed, last_uptime_seconds, time_sync)
            .await;
    }
    if matches!(state.overlay_mode(), OverlayMode::Clock) {
        render_clock_overlay(
            &mut context.inkplate,
            last_uptime_seconds,
            time_sync,
            state.battery_percent,
        )
        .await;
    }
    state.screen_initialized = true;
}

async fn handle_apply_app_state_command_event(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    command: AppStateCommand,
    ack_request_id: Option<u16>,
) {
    let result = state.apply_state_command(context, command).await;
    if let Some(request_id) = ack_request_id {
        APP_STATE_APPLY_ACKS
            .send(AppStateApplyAck {
                request_id,
                snapshot: result.after,
                status: apply_status_code(result.status),
            })
            .await;
    }
    if result.changed() {
        if matches!(result.after.base, BaseMode::TouchWizard) {
            state.touch_wizard = TouchCalibrationWizard::new(state.touch_ready);
            setup_touch_wizard_screen(context, state).await;
            return;
        }
        if !result.after.services.upload_enabled {
            render_active_mode_from_state(context, state, state.last_uptime_seconds).await;
            state.screen_initialized = true;
        }
    }
}
