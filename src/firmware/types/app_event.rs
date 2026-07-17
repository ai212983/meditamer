use super::TimeSyncCommand;
use crate::firmware::app_state::AppStateCommand;
use crate::firmware::{input::gpio36::Gpio36Action, touch::types::TouchStatus};

#[derive(Clone, Copy)]
pub(crate) enum AppEvent {
    Refresh {
        uptime_seconds: u32,
    },
    BatteryTick,
    TimeSync(TimeSyncCommand),
    TouchStatus(TouchStatus),
    Gpio36Action(Gpio36Action),
    ImuActionsReady,
    #[cfg(not(feature = "wifi-debug-slim-app"))]
    StartTouchCalibrationWizard,
    ForceRepaint,
    ForceMarbleRepaint,
    ApplyAppStateCommand {
        command: AppStateCommand,
        ack_request_id: Option<u16>,
    },
}
