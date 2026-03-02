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
