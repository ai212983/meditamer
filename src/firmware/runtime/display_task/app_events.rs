use core::sync::atomic::Ordering;

use embassy_time::{Duration, Instant};

use super::super::super::{
    config::FULL_REFRESH_EVERY_N_UPDATES,
    render::{
        next_visual_seed, render_active_mode, render_battery_update, render_clock_update,
        render_shanshui_update, render_suminagashi_update, render_visual_update,
        sample_battery_percent,
    },
    runtime::service_mode,
    touch::{
        config::{TOUCH_IRQ_BURST_MS, TOUCH_IRQ_LOW, TOUCH_SAMPLE_IDLE_FALLBACK_MS},
        tasks::request_touch_pipeline_reset,
        wizard::{render_touch_wizard_waiting_screen, TouchCalibrationWizard},
    },
    types::{AppEvent, DisplayContext, DisplayMode, TimeSyncState},
};

use super::state::DisplayLoopState;

pub(super) async fn handle_app_event(
    event: AppEvent,
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
) {
    let upload_enabled = service_mode::upload_enabled();
    match event {
        AppEvent::Refresh { uptime_seconds } => {
            state.last_uptime_seconds = uptime_seconds;
            if upload_enabled {
                return;
            }
            if !state.touch_wizard_requested {
                if state.display_mode == DisplayMode::Clock {
                    let do_full_refresh = !state.screen_initialized
                        || state
                            .update_count
                            .is_multiple_of(FULL_REFRESH_EVERY_N_UPDATES);
                    render_clock_update(
                        &mut context.inkplate,
                        uptime_seconds,
                        state.time_sync,
                        state.battery_percent,
                        do_full_refresh,
                    )
                    .await;
                    state.update_count = state.update_count.wrapping_add(1);
                } else {
                    let display_mode = state.display_mode;
                    let time_sync = state.time_sync;
                    render_visual_update(
                        &mut context.inkplate,
                        display_mode,
                        uptime_seconds,
                        time_sync,
                        &mut state.pattern_nonce,
                        &mut state.first_visual_seed_pending,
                    )
                    .await;
                    state.update_count = 0;
                }
                state.screen_initialized = true;
            }
        }
        AppEvent::BatteryTick => {
            if upload_enabled {
                return;
            }
            if let Some(sampled_percent) = sample_battery_percent(&mut context.inkplate) {
                state.battery_percent = Some(sampled_percent);
            }

            if !state.touch_wizard_requested {
                if state.screen_initialized {
                    if state.display_mode == DisplayMode::Clock {
                        render_battery_update(&mut context.inkplate, state.battery_percent).await;
                    }
                } else if state.display_mode == DisplayMode::Clock {
                    let display_mode = state.display_mode;
                    let last_uptime_seconds = state.last_uptime_seconds;
                    let time_sync = state.time_sync;
                    let battery_percent = state.battery_percent;
                    render_active_mode(
                        &mut context.inkplate,
                        display_mode,
                        last_uptime_seconds,
                        time_sync,
                        battery_percent,
                        (
                            &mut state.pattern_nonce,
                            &mut state.first_visual_seed_pending,
                        ),
                        true,
                    )
                    .await;
                    state.screen_initialized = true;
                }
            }
        }
        AppEvent::TimeSync(cmd) => {
            let uptime_now = Instant::now().as_secs().min(u32::MAX as u64) as u32;
            state.last_uptime_seconds = state.last_uptime_seconds.max(uptime_now);
            state.time_sync = Some(TimeSyncState {
                unix_epoch_utc_seconds: cmd.unix_epoch_utc_seconds,
                tz_offset_minutes: cmd.tz_offset_minutes,
                sync_instant: Instant::now(),
            });
            state.update_count = 0;
            if upload_enabled {
                return;
            }
            if !state.touch_wizard_requested {
                let display_mode = state.display_mode;
                let last_uptime_seconds = state.last_uptime_seconds;
                let time_sync = state.time_sync;
                let battery_percent = state.battery_percent;
                render_active_mode(
                    &mut context.inkplate,
                    display_mode,
                    last_uptime_seconds,
                    time_sync,
                    battery_percent,
                    (
                        &mut state.pattern_nonce,
                        &mut state.first_visual_seed_pending,
                    ),
                    true,
                )
                .await;
                state.screen_initialized = true;
            }
        }
        AppEvent::TouchIrq => {
            if upload_enabled {
                return;
            }
            state.touch_irq_pending = state.touch_irq_pending.saturating_add(1);
            let now = Instant::now();
            state.touch_irq_burst_until = now + Duration::from_millis(TOUCH_IRQ_BURST_MS);
            if state.touch_next_sample_at > now {
                state.touch_next_sample_at = now;
            }
            state.touch_idle_fallback_at =
                now + Duration::from_millis(TOUCH_SAMPLE_IDLE_FALLBACK_MS);
        }
        AppEvent::StartTouchCalibrationWizard => {
            if upload_enabled {
                return;
            }
            esp_println::println!(
                "touch_wizard: start_event touch_ready={}",
                state.touch_ready
            );
            state.touch_wizard_requested = true;
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
            if state.touch_ready {
                state.touch_wizard = TouchCalibrationWizard::new(true);
                state.touch_wizard.render_full(&mut context.inkplate).await;
                state.screen_initialized = true;
            } else {
                state.touch_wizard = TouchCalibrationWizard::new(false);
                render_touch_wizard_waiting_screen(&mut context.inkplate).await;
                state.screen_initialized = true;
            }
        }
        AppEvent::ForceRepaint => {
            if upload_enabled {
                return;
            }
            if !state.touch_wizard_requested {
                state.update_count = 0;
                let display_mode = state.display_mode;
                let last_uptime_seconds = state.last_uptime_seconds;
                let time_sync = state.time_sync;
                let battery_percent = state.battery_percent;
                render_active_mode(
                    &mut context.inkplate,
                    display_mode,
                    last_uptime_seconds,
                    time_sync,
                    battery_percent,
                    (
                        &mut state.pattern_nonce,
                        &mut state.first_visual_seed_pending,
                    ),
                    true,
                )
                .await;
                state.screen_initialized = true;
            }
        }
        AppEvent::ForceMarbleRepaint => {
            if upload_enabled {
                return;
            }
            if !state.touch_wizard_requested {
                let last_uptime_seconds = state.last_uptime_seconds;
                let time_sync = state.time_sync;
                let seed = next_visual_seed(
                    last_uptime_seconds,
                    time_sync,
                    &mut state.pattern_nonce,
                    &mut state.first_visual_seed_pending,
                );
                if state.display_mode == DisplayMode::Shanshui {
                    render_shanshui_update(
                        &mut context.inkplate,
                        seed,
                        last_uptime_seconds,
                        time_sync,
                    )
                    .await;
                } else {
                    render_suminagashi_update(
                        &mut context.inkplate,
                        seed,
                        last_uptime_seconds,
                        time_sync,
                    )
                    .await;
                }
                state.screen_initialized = true;
            }
        }
        AppEvent::SetRuntimeServices(services) => {
            service_mode::set_runtime_services(services);
            context.mode_store.save_runtime_services(services);
            if services.upload_enabled_flag() {
                let _ = context.inkplate.frontlight_off();
            }
            esp_println::println!(
                "runtime_mode: upload={} assets={}",
                if services.upload_enabled_flag() {
                    "on"
                } else {
                    "off"
                },
                if services.asset_reads_enabled_flag() {
                    "on"
                } else {
                    "off"
                }
            );
        }
    }
}
