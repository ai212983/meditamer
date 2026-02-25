use core::sync::atomic::Ordering;

#[cfg(feature = "asset-upload-http")]
use embassy_time::Timer;
use embassy_time::{Duration, Instant};

#[cfg(feature = "asset-upload-http")]
use super::super::super::types::RuntimeMode;
use super::super::super::{
    config::FULL_REFRESH_EVERY_N_UPDATES,
    render::{
        next_visual_seed, render_active_mode, render_battery_update, render_clock_update,
        render_shanshui_update, render_suminagashi_update, render_visual_update,
        sample_battery_percent,
    },
    touch::{
        config::{TOUCH_IRQ_BURST_MS, TOUCH_IRQ_LOW, TOUCH_SAMPLE_IDLE_FALLBACK_MS},
        tasks::request_touch_pipeline_reset,
        wizard::{render_touch_wizard_waiting_screen, TouchCalibrationWizard},
    },
    types::{AppEvent, DisplayContext, DisplayMode, TimeSyncState},
};

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_app_event(
    event: AppEvent,
    context: &mut DisplayContext,
    update_count: &mut u32,
    last_uptime_seconds: &mut u32,
    time_sync: &mut Option<TimeSyncState>,
    battery_percent: &mut Option<u8>,
    display_mode: &mut DisplayMode,
    screen_initialized: &mut bool,
    pattern_nonce: &mut u32,
    first_visual_seed_pending: &mut bool,
    touch_wizard_requested: &mut bool,
    touch_ready: bool,
    touch_wizard: &mut TouchCalibrationWizard,
    touch_last_nonzero_at: &mut Option<Instant>,
    touch_irq_pending: &mut u8,
    touch_irq_burst_until: &mut Instant,
    touch_idle_fallback_at: &mut Instant,
    backlight_cycle_start: &mut Option<Instant>,
    backlight_level: &mut u8,
    touch_next_sample_at: &mut Instant,
) {
    match event {
        AppEvent::Refresh { uptime_seconds } => {
            *last_uptime_seconds = uptime_seconds;
            if !*touch_wizard_requested {
                if *display_mode == DisplayMode::Clock {
                    let do_full_refresh = !*screen_initialized
                        || update_count.is_multiple_of(FULL_REFRESH_EVERY_N_UPDATES);
                    render_clock_update(
                        &mut context.inkplate,
                        uptime_seconds,
                        *time_sync,
                        *battery_percent,
                        do_full_refresh,
                    )
                    .await;
                    *update_count = update_count.wrapping_add(1);
                } else {
                    render_visual_update(
                        &mut context.inkplate,
                        *display_mode,
                        uptime_seconds,
                        *time_sync,
                        pattern_nonce,
                        first_visual_seed_pending,
                    )
                    .await;
                    *update_count = 0;
                }
                *screen_initialized = true;
            }
        }
        AppEvent::BatteryTick => {
            if let Some(sampled_percent) = sample_battery_percent(&mut context.inkplate) {
                *battery_percent = Some(sampled_percent);
            }

            if !*touch_wizard_requested {
                if *screen_initialized {
                    if *display_mode == DisplayMode::Clock {
                        render_battery_update(&mut context.inkplate, *battery_percent).await;
                    }
                } else if *display_mode == DisplayMode::Clock {
                    render_active_mode(
                        &mut context.inkplate,
                        *display_mode,
                        *last_uptime_seconds,
                        *time_sync,
                        *battery_percent,
                        (pattern_nonce, first_visual_seed_pending),
                        true,
                    )
                    .await;
                    *screen_initialized = true;
                }
            }
        }
        AppEvent::TimeSync(cmd) => {
            let uptime_now = Instant::now().as_secs().min(u32::MAX as u64) as u32;
            *last_uptime_seconds = (*last_uptime_seconds).max(uptime_now);
            *time_sync = Some(TimeSyncState {
                unix_epoch_utc_seconds: cmd.unix_epoch_utc_seconds,
                tz_offset_minutes: cmd.tz_offset_minutes,
                sync_instant: Instant::now(),
            });
            *update_count = 0;
            if !*touch_wizard_requested {
                render_active_mode(
                    &mut context.inkplate,
                    *display_mode,
                    *last_uptime_seconds,
                    *time_sync,
                    *battery_percent,
                    (pattern_nonce, first_visual_seed_pending),
                    true,
                )
                .await;
                *screen_initialized = true;
            }
        }
        AppEvent::TouchIrq => {
            *touch_irq_pending = touch_irq_pending.saturating_add(1);
            let now = Instant::now();
            *touch_irq_burst_until = now + Duration::from_millis(TOUCH_IRQ_BURST_MS);
            if *touch_next_sample_at > now {
                *touch_next_sample_at = now;
            }
            *touch_idle_fallback_at = now + Duration::from_millis(TOUCH_SAMPLE_IDLE_FALLBACK_MS);
        }
        AppEvent::StartTouchCalibrationWizard => {
            esp_println::println!("touch_wizard: start_event touch_ready={}", touch_ready);
            *touch_wizard_requested = true;
            *touch_last_nonzero_at = None;
            *touch_irq_pending = 0;
            *touch_irq_burst_until = Instant::now();
            TOUCH_IRQ_LOW.store(false, Ordering::Relaxed);
            *touch_idle_fallback_at =
                Instant::now() + Duration::from_millis(TOUCH_SAMPLE_IDLE_FALLBACK_MS);
            *backlight_cycle_start = None;
            *backlight_level = 0;
            let _ = context.inkplate.frontlight_off();
            request_touch_pipeline_reset();
            *touch_next_sample_at = Instant::now();
            if touch_ready {
                *touch_wizard = TouchCalibrationWizard::new(true);
                touch_wizard.render_full(&mut context.inkplate).await;
                *screen_initialized = true;
            } else {
                *touch_wizard = TouchCalibrationWizard::new(false);
                render_touch_wizard_waiting_screen(&mut context.inkplate).await;
                *screen_initialized = true;
            }
        }
        AppEvent::ForceRepaint => {
            if !*touch_wizard_requested {
                *update_count = 0;
                render_active_mode(
                    &mut context.inkplate,
                    *display_mode,
                    *last_uptime_seconds,
                    *time_sync,
                    *battery_percent,
                    (pattern_nonce, first_visual_seed_pending),
                    true,
                )
                .await;
                *screen_initialized = true;
            }
        }
        AppEvent::ForceMarbleRepaint => {
            if !*touch_wizard_requested {
                let seed = next_visual_seed(
                    *last_uptime_seconds,
                    *time_sync,
                    pattern_nonce,
                    first_visual_seed_pending,
                );
                if *display_mode == DisplayMode::Shanshui {
                    render_shanshui_update(
                        &mut context.inkplate,
                        seed,
                        *last_uptime_seconds,
                        *time_sync,
                    )
                    .await;
                } else {
                    render_suminagashi_update(
                        &mut context.inkplate,
                        seed,
                        *last_uptime_seconds,
                        *time_sync,
                    )
                    .await;
                }
                *screen_initialized = true;
            }
        }
        #[cfg(feature = "asset-upload-http")]
        AppEvent::SwitchRuntimeMode(mode) => {
            context.mode_store.save_runtime_mode(mode);
            let _ = context.inkplate.frontlight_off();
            esp_println::println!(
                "runtime_mode: switching_to={}",
                match mode {
                    RuntimeMode::Normal => "normal",
                    RuntimeMode::Upload => "upload",
                }
            );
            Timer::after_millis(100).await;
            esp_hal::system::software_reset();
        }
    }
}
