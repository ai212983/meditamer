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
