use crate::firmware::types::DisplayContext;

use super::super::state::DisplayLoopState;

pub(super) async fn handle_ui_cycle_step_event(
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
        super::super::presentation::handle_ui_cycle_step(context, &mut state.presentation).await
    };
    console::println!(
        "UI_CYCLE_STEP request={} status={:?}",
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
        console::println!(
            "UI_CYCLE_STEP request={} status=AckQueueFull",
            ack_request_id
        );
    }
}
