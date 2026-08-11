use super::apply_state::handle_apply_app_state_command_event;
use super::lifecycle::{handle_battery_tick_event, handle_touch_status_event};
use super::repaint::handle_force_repaint_event;
use super::ui_cycle::handle_ui_cycle_step_event;
use crate::firmware::types::{AppEvent, DisplayContext};

use super::super::gpio36_feedback::handle_gpio36_action;
use super::super::state::DisplayLoopState;

pub(in crate::firmware::runtime::display_task) async fn handle_app_event(
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
        AppEvent::UiCycleStep { ack_request_id } => {
            handle_ui_cycle_step_event(context, state, ack_request_id).await;
        }
        #[cfg(feature = "ui-provider-fixture")]
        AppEvent::UiProviderFixtureStep { ack_request_id } => {
            handle_ui_provider_fixture_step_event(context, state, ack_request_id).await;
        }
        AppEvent::ApplyAppStateCommand {
            command,
            ack_request_id,
        } => {
            handle_apply_app_state_command_event(context, state, command, ack_request_id).await;
        }
    }
}

#[cfg(feature = "ui-provider-fixture")]
async fn handle_ui_provider_fixture_step_event(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    ack_request_id: u16,
) {
    let status = if matches!(
        state.gpio36_mode,
        crate::firmware::input::gpio36::Gpio36Mode::ButtonOnly
    ) {
        crate::firmware::types::UiCycleStepStatus::Busy
    } else {
        super::super::lvgl::handle_ui_provider_fixture_step(context, &mut state.lvgl).await
    };
    esp_println::println!(
        "UI_PROVIDER_FIXTURE_STEP request={} status={:?}",
        ack_request_id,
        status
    );
    let ack = crate::firmware::types::UiCycleStepAck {
        request_id: ack_request_id,
        status,
    };
    if crate::firmware::config::UI_CYCLE_STEP_ACKS
        .try_send(ack)
        .is_err()
    {
        esp_println::println!(
            "UI_PROVIDER_FIXTURE_STEP request={} status=AckQueueFull",
            ack_request_id
        );
    }
}
