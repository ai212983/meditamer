use super::TimeSyncCommand;
use crate::firmware::app_state::AppStateCommand;

#[derive(Clone, Copy)]
pub(crate) enum AppEvent {
    Refresh {
        uptime_seconds: u32,
    },
    BatteryTick,
    TimeSync(TimeSyncCommand),
    TouchIrq,
    StartTouchCalibrationWizard,
    ForceRepaint,
    ForceMarbleRepaint,
    ApplyAppStateCommand {
        command: AppStateCommand,
        ack_request_id: Option<u16>,
    },
}
