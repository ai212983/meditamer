use crate::firmware::app_state::AppStateCommand;
use crate::firmware::{input::gpio36::Gpio36Action, touch::types::TouchStatus};

#[derive(Clone, Copy)]
pub(crate) enum AppEvent {
    BatteryTick,
    TouchStatus(TouchStatus),
    Gpio36Action(Gpio36Action),
    ImuActionsReady,
    ForceRepaint,
    UiCycleStep {
        ack_request_id: u16,
    },
    #[cfg(feature = "ui-provider-fixture")]
    UiProviderFixtureStep {
        ack_request_id: u16,
    },
    ApplyAppStateCommand {
        command: AppStateCommand,
        ack_request_id: Option<u16>,
    },
}
