use crate::firmware::app_state::AppStateCommand;
use crate::firmware::{input::gpio36::Gpio36Action, touch::types::TouchStatus};

#[derive(Clone, Copy)]
pub(crate) enum AppEvent {
    BatteryTick,
    TouchStatus(TouchStatus),
    Gpio36Action(Gpio36Action),
    ImuActionsReady,
    ForceRepaint,
    ApplyAppStateCommand {
        command: AppStateCommand,
        ack_request_id: Option<u16>,
    },
}
