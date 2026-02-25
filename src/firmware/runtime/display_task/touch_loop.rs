use core::sync::atomic::Ordering;

use embassy_time::{Duration, Instant, Timer};

use super::super::super::{
    render::render_active_mode,
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
    types::{DisplayContext, DisplayMode, TimeSyncState},
};

#[allow(clippy::too_many_arguments)]
pub(super) async fn process_touch_cycle(
    context: &mut DisplayContext,
    touch_ready: &mut bool,
    touch_retry_at: &mut Instant,
    touch_next_sample_at: &mut Instant,
    touch_contact_active: &mut bool,
    touch_last_nonzero_at: &mut Option<Instant>,
    touch_irq_pending: &mut u8,
    touch_irq_burst_until: &mut Instant,
    touch_idle_fallback_at: &mut Instant,
    touch_wizard_trace_capture_until_ms: &mut u64,
    touch_wizard_requested: &mut bool,
    touch_wizard: &mut TouchCalibrationWizard,
    screen_initialized: &mut bool,
    touch_feedback_dirty: &mut bool,
    touch_feedback_next_flush_at: &mut Instant,
    backlight_cycle_start: &mut Option<Instant>,
    backlight_level: &mut u8,
    update_count: &mut u32,
    display_mode: &mut DisplayMode,
    last_uptime_seconds: u32,
    time_sync: Option<TimeSyncState>,
    battery_percent: Option<u8>,
    pattern_nonce: &mut u32,
    first_visual_seed_pending: &mut bool,
    trace_epoch: Instant,
) {
    if !*touch_ready && Instant::now() >= *touch_retry_at {
        *touch_ready = try_touch_init_with_logs(&mut context.inkplate, "retry");
        if *touch_ready {
            request_touch_pipeline_reset();
            *touch_irq_pending = 0;
            *touch_irq_burst_until = Instant::now();
            TOUCH_IRQ_LOW.store(false, Ordering::Relaxed);
            *touch_next_sample_at = Instant::now();
            *touch_idle_fallback_at =
                Instant::now() + Duration::from_millis(TOUCH_SAMPLE_IDLE_FALLBACK_MS);
            if *touch_wizard_requested && !touch_wizard.is_active() {
                *touch_wizard = TouchCalibrationWizard::new(true);
                touch_wizard.render_full(&mut context.inkplate).await;
                *screen_initialized = true;
            }
        } else {
            *touch_retry_at = Instant::now() + Duration::from_millis(TOUCH_INIT_RETRY_MS);
        }
    }

    let mut sampled_touch_count = 0u8;
    while *touch_ready
        && sampled_touch_count < TOUCH_MAX_CATCHUP_SAMPLES
        && Instant::now() >= *touch_next_sample_at
    {
        let scheduled_sample_at = *touch_next_sample_at;
        let sample_instant = Instant::now();
        let touch_recent_nonzero = touch_last_nonzero_at.is_some_and(|last_nonzero_at| {
            sample_instant
                .saturating_duration_since(last_nonzero_at)
                .as_millis()
                <= TOUCH_ZERO_CONFIRM_WINDOW_MS
        });
        let touch_irq_low = TOUCH_IRQ_LOW.load(Ordering::Relaxed);
        let irq_burst_active = sample_instant <= *touch_irq_burst_until;
        let idle_poll_due = sample_instant >= *touch_idle_fallback_at;
        let should_sample = *touch_irq_pending > 0
            || touch_irq_low
            || irq_burst_active
            || *touch_contact_active
            || touch_recent_nonzero
            || idle_poll_due;
        if !should_sample {
            *touch_next_sample_at = *touch_idle_fallback_at;
            break;
        }
        if *touch_irq_pending > 0 {
            *touch_irq_pending = touch_irq_pending.saturating_sub(1);
        }

        match context.inkplate.touch_read_sample(0) {
            Ok(sample) => {
                if sample.touch_count > 0 {
                    *touch_last_nonzero_at = Some(sample_instant);
                    *touch_irq_burst_until =
                        sample_instant + Duration::from_millis(TOUCH_IRQ_BURST_MS);
                } else if let Some(last_nonzero_at) = *touch_last_nonzero_at {
                    if sample_instant
                        .saturating_duration_since(last_nonzero_at)
                        .as_millis()
                        > TOUCH_ZERO_CONFIRM_WINDOW_MS
                    {
                        *touch_last_nonzero_at = None;
                    }
                }

                // Use scheduled sample time for gesture timing. If we are catching up,
                // multiple reads can happen back-to-back in real time; using `Instant::now()`
                // for each would collapse debounce durations and miss quick taps.
                let t_ms = scheduled_sample_at
                    .saturating_duration_since(trace_epoch)
                    .as_millis();
                if touch_wizard.is_active() {
                    let raw_present = sample.raw[7].count_ones() > 0;
                    if sample.touch_count > 0 || raw_present {
                        *touch_wizard_trace_capture_until_ms =
                            t_ms.saturating_add(TOUCH_WIZARD_TRACE_CAPTURE_TAIL_MS);
                    }
                    if t_ms <= *touch_wizard_trace_capture_until_ms {
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
                *touch_ready = false;
                *touch_last_nonzero_at = None;
                *touch_irq_pending = 0;
                *touch_irq_burst_until = Instant::now();
                TOUCH_IRQ_LOW.store(false, Ordering::Relaxed);
                let _ = context.inkplate.touch_shutdown();
                *touch_retry_at = sample_instant + Duration::from_millis(TOUCH_INIT_RETRY_MS);
                esp_println::println!("touch: read_error; retrying");
                request_touch_pipeline_reset();
                if *touch_wizard_requested {
                    *touch_wizard = TouchCalibrationWizard::new(false);
                    render_touch_wizard_waiting_screen(&mut context.inkplate).await;
                    *screen_initialized = true;
                }
                break;
            }
        }

        let touch_recent_nonzero = touch_last_nonzero_at.is_some_and(|last_nonzero_at| {
            sample_instant
                .saturating_duration_since(last_nonzero_at)
                .as_millis()
                <= TOUCH_ZERO_CONFIRM_WINDOW_MS
        });
        if !*touch_contact_active && !touch_recent_nonzero && *touch_irq_pending == 0 {
            *touch_idle_fallback_at =
                sample_instant + Duration::from_millis(TOUCH_SAMPLE_IDLE_FALLBACK_MS);
        }
        let sample_period_ms = next_touch_sample_period_ms(
            *touch_contact_active
                || touch_recent_nonzero
                || *touch_irq_pending > 0
                || touch_irq_low
                || sample_instant <= *touch_irq_burst_until,
        );
        sampled_touch_count = sampled_touch_count.saturating_add(1);
        *touch_next_sample_at = scheduled_sample_at + Duration::from_millis(sample_period_ms);
    }

    while let Ok(touch_event) = TOUCH_PIPELINE_EVENTS.try_receive() {
        match touch_event.kind {
            TouchEventKind::Down | TouchEventKind::Move | TouchEventKind::LongPress => {
                *touch_contact_active = true;
            }
            TouchEventKind::Up
            | TouchEventKind::Tap
            | TouchEventKind::Swipe(_)
            | TouchEventKind::Cancel => {
                *touch_contact_active = false;
                if touch_last_nonzero_at.is_none() {
                    *touch_idle_fallback_at =
                        Instant::now() + Duration::from_millis(TOUCH_SAMPLE_IDLE_FALLBACK_MS);
                }
            }
        }

        if touch_wizard.is_active() {
            if TOUCH_FEEDBACK_ENABLED
                && matches!(
                    touch_event.kind,
                    TouchEventKind::Down | TouchEventKind::Move
                )
            {
                draw_touch_feedback_dot(&mut context.inkplate, touch_event.x, touch_event.y);
                *touch_feedback_dirty = true;
            }

            match touch_wizard
                .handle_event(&mut context.inkplate, touch_event)
                .await
            {
                WizardDispatch::Inactive => {}
                WizardDispatch::Consumed => continue,
                WizardDispatch::Finished => {
                    *touch_wizard_requested = false;
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
                    continue;
                }
            }
        }

        handle_touch_event(
            touch_event,
            context,
            TouchEventContext {
                touch_feedback_dirty,
                backlight_cycle_start,
                backlight_level,
                update_count,
                display_mode,
                last_uptime_seconds,
                time_sync,
                battery_percent,
                seed_state: (pattern_nonce, first_visual_seed_pending),
                screen_initialized,
            },
        )
        .await;
    }

    // Flush feedback after sampling and event handling so rendering never blocks
    // the beginning of the touch-capture window.
    if *touch_feedback_dirty
        && !*touch_contact_active
        && Instant::now() >= *touch_feedback_next_flush_at
    {
        let _ = context.inkplate.display_bw_partial_async(false).await;
        *touch_feedback_dirty = false;
        *touch_feedback_next_flush_at =
            Instant::now() + Duration::from_millis(TOUCH_FEEDBACK_MIN_REFRESH_MS);
    }
}
