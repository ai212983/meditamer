async fn handle_touch_status_event(
    status: TouchStatus,
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
) {
    match status {
        TouchStatus::Ready { .. } => {
            state.touch_ready = true;
            if state.in_touch_wizard_mode() && !state.touch_wizard.is_active() {
                setup_touch_wizard_screen(context, state).await;
            }
            state.touch_startup_settled = true;
        }
        TouchStatus::Initializing => {
            state.touch_ready = false;
            state.touch_contact_active = false;
            state.touch_last_nonzero_at = None;
            if state.in_touch_wizard_mode() {
                state.touch_wizard = TouchCalibrationWizard::new(false);
                render_touch_wizard_waiting_screen(&mut context.inkplate).await;
                state.screen_initialized = true;
            }
        }
        TouchStatus::Fault => {
            state.touch_ready = false;
            state.touch_contact_active = false;
            state.touch_last_nonzero_at = None;
            if state.in_touch_wizard_mode() {
                state.touch_wizard = TouchCalibrationWizard::new(false);
                render_touch_wizard_waiting_screen(&mut context.inkplate).await;
                state.screen_initialized = true;
            }
            state.touch_startup_settled = true;
        }
    }
}

async fn handle_start_touch_calibration_wizard_event(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    upload_enabled: bool,
) {
    if cfg!(feature = "wifi-debug-slim-app") || upload_enabled {
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
    state.backlight_cycle_start = None;
    state.backlight_level = 0;
    let _ = context.inkplate.frontlight_off().await;
    request_touch_pipeline_reset();
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
