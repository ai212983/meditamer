pub(super) async fn handle_app_event(
    event: AppEvent,
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
) {
    let upload_enabled = state.upload_enabled();
    match event {
        AppEvent::BatteryTick => {
            handle_battery_tick_event(context, upload_enabled).await;
        }
        AppEvent::TouchStatus(status) => {
            handle_touch_status_event(status, context, state).await;
        }
        AppEvent::Gpio36Action(action) => {
            handle_gpio36_action(action, context, state).await;
        }
        AppEvent::ImuActionsReady => {}
        AppEvent::ForceRepaint => {
            handle_force_repaint_event(context, state, upload_enabled).await;
        }
        AppEvent::ApplyAppStateCommand {
            command,
            ack_request_id,
        } => {
            handle_apply_app_state_command_event(context, state, command, ack_request_id).await;
        }
    }
}
