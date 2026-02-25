use super::RuntimeServicesUpdate;
use super::TimeSyncCommand;

#[derive(Clone, Copy)]
pub(crate) enum AppEvent {
    Refresh { uptime_seconds: u32 },
    BatteryTick,
    TimeSync(TimeSyncCommand),
    TouchIrq,
    StartTouchCalibrationWizard,
    ForceRepaint,
    ForceMarbleRepaint,
    UpdateRuntimeServices(RuntimeServicesUpdate),
}
