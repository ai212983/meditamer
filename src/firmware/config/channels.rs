use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};

#[cfg(feature = "asset-upload-http")]
use super::super::types::WifiCredentials;
use super::super::types::{
    AppEvent, AppStateApplyAck, SdPowerRequest, SdRequest, SdResult, SdUploadRequest,
    SdUploadResult, SerialStatusEvent, TapTraceSample, UiCycleStepAck, WallClockQueryResult,
};
#[cfg(feature = "asset-upload-http")]
use super::super::types::{
    NetConfigSet, NetControlCommand, WifiConfigRequest, WifiConfigResponse, WifiRuntimePolicy,
};
use crate::firmware::app_state::AppStateDiagControl;

pub(crate) static APP_EVENTS: Channel<CriticalSectionRawMutex, AppEvent, 8> = Channel::new();
pub(crate) static SD_REQUESTS: Channel<CriticalSectionRawMutex, SdRequest, 8> = Channel::new();
pub(crate) static SD_RESULTS: Channel<CriticalSectionRawMutex, SdResult, 16> = Channel::new();
// The FAT runner yields at every I/O boundary, allowing the serial task to
// drain this diagnostic queue between storage actions.
pub(crate) static SD_SERIAL_LINES: Channel<CriticalSectionRawMutex, heapless::String<256>, 8> =
    Channel::new();
// Runtime producers never write UART directly. Events are lossy diagnostics, so producers use
// try_send and cannot delay acquisition tasks while the serial task handles a command.
pub(crate) static SERIAL_STATUS_EVENTS: Channel<CriticalSectionRawMutex, SerialStatusEvent, 8> =
    Channel::new();
pub(crate) static SD_DIAG_RESULTS: Channel<CriticalSectionRawMutex, SdResult, 8> = Channel::new();
pub(crate) static DIAG_CONTROL_EVENTS: Channel<CriticalSectionRawMutex, AppStateDiagControl, 4> =
    Channel::new();
pub(crate) static SD_UPLOAD_REQUESTS: Channel<CriticalSectionRawMutex, SdUploadRequest, 2> =
    Channel::new();
pub(crate) static SD_UPLOAD_RESULTS: Channel<CriticalSectionRawMutex, SdUploadResult, 2> =
    Channel::new();
#[cfg(feature = "asset-upload-http")]
pub(crate) static WIFI_CREDENTIALS_UPDATES: Channel<CriticalSectionRawMutex, WifiCredentials, 2> =
    Channel::new();
#[cfg(feature = "asset-upload-http")]
pub(crate) static WIFI_RUNTIME_POLICY_UPDATES: Channel<
    CriticalSectionRawMutex,
    WifiRuntimePolicy,
    2,
> = Channel::new();
#[cfg(feature = "asset-upload-http")]
pub(crate) static NET_CONFIG_SET_UPDATES: Channel<CriticalSectionRawMutex, NetConfigSet, 2> =
    Channel::new();
#[cfg(feature = "asset-upload-http")]
pub(crate) static NET_CONTROL_COMMANDS: Channel<CriticalSectionRawMutex, NetControlCommand, 2> =
    Channel::new();
#[cfg(feature = "asset-upload-http")]
pub(crate) static WIFI_CONFIG_REQUESTS: Channel<CriticalSectionRawMutex, WifiConfigRequest, 1> =
    Channel::new();
#[cfg(feature = "asset-upload-http")]
pub(crate) static WIFI_CONFIG_RESPONSES: Channel<CriticalSectionRawMutex, WifiConfigResponse, 1> =
    Channel::new();
pub(crate) static SD_POWER_REQUESTS: Channel<CriticalSectionRawMutex, SdPowerRequest, 2> =
    Channel::new();
pub(crate) static SD_POWER_RESPONSES: Channel<CriticalSectionRawMutex, bool, 2> = Channel::new();
pub(crate) static APP_STATE_APPLY_ACKS: Channel<CriticalSectionRawMutex, AppStateApplyAck, 2> =
    Channel::new();
pub(crate) static UI_CYCLE_STEP_ACKS: Channel<CriticalSectionRawMutex, UiCycleStepAck, 2> =
    Channel::new();
pub(crate) static TAP_TRACE_SAMPLES: Channel<CriticalSectionRawMutex, TapTraceSample, 8> =
    Channel::new();
// The Ambient Home screen (display task) requests fresh RTC reads from the
// serial task, the sole owner of RTC I2C access. No result is cached here:
// each request produces exactly one fresh transaction.
pub(crate) static WALL_CLOCK_REQUESTS: Channel<CriticalSectionRawMutex, (), 2> = Channel::new();
pub(crate) static WALL_CLOCK_RESPONSES: Channel<CriticalSectionRawMutex, WallClockQueryResult, 2> =
    Channel::new();
