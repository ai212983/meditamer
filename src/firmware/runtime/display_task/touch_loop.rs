use core::sync::atomic::Ordering;

use embassy_time::{Duration, Instant, Timer};

use super::super::super::{
    app_state::{AppStateCommand, BaseMode},
    render::{render_active_mode, RenderActiveParams},
    touch::{
        config::{
            TOUCH_FEEDBACK_ENABLED, TOUCH_FEEDBACK_MIN_REFRESH_MS, TOUCH_INIT_RETRY_MS,
            TOUCH_IRQ_BURST_MS, TOUCH_IRQ_LOW, TOUCH_MAX_CATCHUP_SAMPLES, TOUCH_PIPELINE_EVENTS,
            TOUCH_SAMPLE_IDLE_FALLBACK_MS, TOUCH_WIZARD_RAW_TRACE_SAMPLES,
            TOUCH_WIZARD_TRACE_CAPTURE_TAIL_MS, TOUCH_ZERO_CONFIRM_WINDOW_MS,
        },
        integration::{handle_touch_event, TouchEventContext},
        tasks::{
            draw_touch_feedback_dot, next_touch_sample_period_ms, push_touch_input_sample,
            request_touch_pipeline_reset, try_touch_init_with_logs,
        },
        types::{TouchEventKind, TouchSampleFrame, TouchTraceSample},
        wizard::{render_touch_wizard_waiting_screen, TouchCalibrationWizard, WizardDispatch},
    },
    types::DisplayContext,
};

use super::state::DisplayLoopState;

pub(super) async fn process_touch_cycle(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
) {
    if !state.touch_ready && Instant::now() >= state.touch_retry_at {
        state.touch_ready = try_touch_init_with_logs(&mut context.inkplate, "retry");
        if state.touch_ready {
            request_touch_pipeline_reset();
            state.touch_irq_pending = 0;
            state.touch_irq_burst_until = Instant::now();
            TOUCH_IRQ_LOW.store(false, Ordering::Relaxed);
            state.touch_next_sample_at = Instant::now();
            state.touch_idle_fallback_at =
                Instant::now() + Duration::from_millis(TOUCH_SAMPLE_IDLE_FALLBACK_MS);
            if state.in_touch_wizard_mode() && !state.touch_wizard.is_active() {
                state.touch_wizard = TouchCalibrationWizard::new(true);
                state.touch_wizard.render_full(&mut context.inkplate).await;
                state.screen_initialized = true;
            }
        } else {
            state.touch_retry_at = Instant::now() + Duration::from_millis(TOUCH_INIT_RETRY_MS);
        }
    }

    let mut sampled_touch_count = 0u8;
    while state.touch_ready
        && sampled_touch_count < TOUCH_MAX_CATCHUP_SAMPLES
        && Instant::now() >= state.touch_next_sample_at
    {
        let scheduled_sample_at = state.touch_next_sample_at;
        let sample_instant = Instant::now();
        let touch_recent_nonzero = state.touch_last_nonzero_at.is_some_and(|last_nonzero_at| {
            sample_instant
                .saturating_duration_since(last_nonzero_at)
                .as_millis()
                <= TOUCH_ZERO_CONFIRM_WINDOW_MS
        });
        let touch_irq_low = TOUCH_IRQ_LOW.load(Ordering::Relaxed);
        let irq_burst_active = sample_instant <= state.touch_irq_burst_until;
        let idle_poll_due = sample_instant >= state.touch_idle_fallback_at;
        let should_sample = state.touch_irq_pending > 0
            || touch_irq_low
            || irq_burst_active
            || state.touch_contact_active
            || touch_recent_nonzero
            || idle_poll_due;
        if !should_sample {
            state.touch_next_sample_at = state.touch_idle_fallback_at;
            break;
        }
        if state.touch_irq_pending > 0 {
            state.touch_irq_pending = state.touch_irq_pending.saturating_sub(1);
        }

        match context.inkplate.touch_read_sample(0) {
            Ok(sample) => {
                if sample.touch_count > 0 {
                    state.touch_last_nonzero_at = Some(sample_instant);
                    state.touch_irq_burst_until =
                        sample_instant + Duration::from_millis(TOUCH_IRQ_BURST_MS);
                } else if let Some(last_nonzero_at) = state.touch_last_nonzero_at {
                    if sample_instant
                        .saturating_duration_since(last_nonzero_at)
                        .as_millis()
                        > TOUCH_ZERO_CONFIRM_WINDOW_MS
                    {
                        state.touch_last_nonzero_at = None;
                    }
                }

                // Use scheduled sample time for gesture timing. If we are catching up,
                // multiple reads can happen back-to-back in real time; using `Instant::now()`
                // for each would collapse debounce durations and miss quick taps.
                let t_ms = scheduled_sample_at
                    .saturating_duration_since(state.trace_epoch)
                    .as_millis();
                if state.touch_wizard.is_active() {
                    let raw_present = sample.raw[7].count_ones() > 0;
                    if sample.touch_count > 0 || raw_present {
                        state.touch_wizard_trace_capture_until_ms =
                            t_ms.saturating_add(TOUCH_WIZARD_TRACE_CAPTURE_TAIL_MS);
                    }
                    if t_ms <= state.touch_wizard_trace_capture_until_ms {
                        let _ = TOUCH_WIZARD_RAW_TRACE_SAMPLES
                            .try_send(TouchTraceSample::from_sample(t_ms, sample));
                    }
                }
                push_touch_input_sample(TouchSampleFrame { t_ms, sample }).await;
                // Always yield between capture iterations so touch pipeline task can run
                // even when channel has spare capacity.
                Timer::after_micros(0).await;
            }
            Err(_) => {
                state.touch_ready = false;
                state.touch_last_nonzero_at = None;
                state.touch_irq_pending = 0;
                state.touch_irq_burst_until = Instant::now();
                TOUCH_IRQ_LOW.store(false, Ordering::Relaxed);
                let _ = context.inkplate.touch_shutdown();
                state.touch_retry_at = sample_instant + Duration::from_millis(TOUCH_INIT_RETRY_MS);
                esp_println::println!("touch: read_error; retrying");
                request_touch_pipeline_reset();
                if state.in_touch_wizard_mode() {
                    state.touch_wizard = TouchCalibrationWizard::new(false);
                    render_touch_wizard_waiting_screen(&mut context.inkplate).await;
                    state.screen_initialized = true;
                }
                break;
            }
        }

        let touch_recent_nonzero = state.touch_last_nonzero_at.is_some_and(|last_nonzero_at| {
            sample_instant
                .saturating_duration_since(last_nonzero_at)
                .as_millis()
                <= TOUCH_ZERO_CONFIRM_WINDOW_MS
        });
        if !state.touch_contact_active && !touch_recent_nonzero && state.touch_irq_pending == 0 {
            state.touch_idle_fallback_at =
                sample_instant + Duration::from_millis(TOUCH_SAMPLE_IDLE_FALLBACK_MS);
        }
        let sample_period_ms = next_touch_sample_period_ms(
            state.touch_contact_active
                || touch_recent_nonzero
                || state.touch_irq_pending > 0
                || touch_irq_low
                || sample_instant <= state.touch_irq_burst_until,
        );
        sampled_touch_count = sampled_touch_count.saturating_add(1);
        state.touch_next_sample_at = scheduled_sample_at + Duration::from_millis(sample_period_ms);
    }

    while let Ok(touch_event) = TOUCH_PIPELINE_EVENTS.try_receive() {
        match touch_event.kind {
            TouchEventKind::Down | TouchEventKind::Move | TouchEventKind::LongPress => {
                state.touch_contact_active = true;
            }
            TouchEventKind::Up
            | TouchEventKind::Tap
            | TouchEventKind::Swipe(_)
            | TouchEventKind::Cancel => {
                state.touch_contact_active = false;
                if state.touch_last_nonzero_at.is_none() {
                    state.touch_idle_fallback_at =
                        Instant::now() + Duration::from_millis(TOUCH_SAMPLE_IDLE_FALLBACK_MS);
                }
            }
        }

        if state.touch_wizard.is_active() {
            if TOUCH_FEEDBACK_ENABLED
                && matches!(
                    touch_event.kind,
                    TouchEventKind::Down | TouchEventKind::Move
                )
            {
                draw_touch_feedback_dot(&mut context.inkplate, touch_event.x, touch_event.y);
                state.touch_feedback_dirty = true;
            }

            match state
                .touch_wizard
                .handle_event(&mut context.inkplate, touch_event)
                .await
            {
                WizardDispatch::Inactive => {}
                WizardDispatch::Consumed => continue,
                WizardDispatch::Finished => {
                    let _ = state
                        .apply_state_command(context, AppStateCommand::SetBase(BaseMode::Day))
                        .await;
                    state.update_count = 0;
                    let last_uptime_seconds = state.last_uptime_seconds;
                    let time_sync = state.time_sync;
                    let battery_percent = state.battery_percent;
                    render_active_mode(
                        &mut context.inkplate,
                        RenderActiveParams {
                            base_mode: state.base_mode(),
                            day_background: state.day_background(),
                            overlay_mode: state.overlay_mode(),
                            uptime_seconds: last_uptime_seconds,
                            time_sync,
                            battery_percent,
                            pattern_nonce: &mut state.pattern_nonce,
                            first_visual_seed_pending: &mut state.first_visual_seed_pending,
                        },
                    )
                    .await;
                    state.screen_initialized = true;
                    continue;
                }
            }
        }

        let last_uptime_seconds = state.last_uptime_seconds;
        let time_sync = state.time_sync;
        let battery_percent = state.battery_percent;
        let base_mode = state.base_mode();
        let day_background = state.day_background();
        let overlay_mode = state.overlay_mode();
        if let Some(command) = handle_touch_event(
            touch_event,
            context,
            TouchEventContext {
                touch_feedback_dirty: &mut state.touch_feedback_dirty,
                backlight_cycle_start: &mut state.backlight_cycle_start,
                backlight_level: &mut state.backlight_level,
                update_count: &mut state.update_count,
                base_mode,
                day_background,
                overlay_mode,
                last_uptime_seconds,
                time_sync,
                battery_percent,
                seed_state: (
                    &mut state.pattern_nonce,
                    &mut state.first_visual_seed_pending,
                ),
                screen_initialized: &mut state.screen_initialized,
            },
        )
        .await
        {
            let result = state.apply_state_command(context, command).await;
            if result.changed() && !state.in_touch_wizard_mode() {
                render_active_mode(
                    &mut context.inkplate,
                    RenderActiveParams {
                        base_mode: state.base_mode(),
                        day_background: state.day_background(),
                        overlay_mode: state.overlay_mode(),
                        uptime_seconds: state.last_uptime_seconds,
                        time_sync: state.time_sync,
                        battery_percent: state.battery_percent,
                        pattern_nonce: &mut state.pattern_nonce,
                        first_visual_seed_pending: &mut state.first_visual_seed_pending,
                    },
                )
                .await;
                state.screen_initialized = true;
            }
        }
    }

    // Flush feedback after sampling and event handling so rendering never blocks
    // the beginning of the touch-capture window.
    if state.touch_feedback_dirty
        && !state.touch_contact_active
        && Instant::now() >= state.touch_feedback_next_flush_at
    {
        let _ = context.inkplate.display_bw_partial_async(false).await;
        state.touch_feedback_dirty = false;
        state.touch_feedback_next_flush_at =
            Instant::now() + Duration::from_millis(TOUCH_FEEDBACK_MIN_REFRESH_MS);
    }
}
