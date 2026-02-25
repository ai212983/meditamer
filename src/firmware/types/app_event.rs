#[cfg(feature = "asset-upload-http")]
use super::RuntimeMode;
use super::TimeSyncCommand;

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
    #[cfg(feature = "asset-upload-http")]
    SwitchRuntimeMode(RuntimeMode),
}
