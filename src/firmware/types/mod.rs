mod app_event;
mod base;
mod modes;
mod sd;
mod serial;
mod trace;
mod ui_cycle;
mod wall_clock;
#[cfg(feature = "asset-upload-http")]
mod wifi;

pub(crate) use app_event::AppEvent;
pub(crate) use base::{
    DisplayContext, InkplateDriver, InkplateImuDriver, InkplateRtcDriver, InkplateTouchDriver,
    PanelPinHold, SdProbeDriver, SerialUart, SharedI2cDevice, SD_PATH_MAX, SD_UPLOAD_CHUNK_MAX,
    SD_WRITE_MAX,
};
#[cfg(feature = "asset-upload-http")]
pub(crate) use base::{
    HTTP_INGRESS_ADAPTIVE_FAIRNESS, HTTP_INGRESS_COOP_YIELD_BYTES, HTTP_INGRESS_COOP_YIELD_READS,
    HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS, HTTP_RX_BUF_TARGET_BYTES, WIFI_CONFIG_FILE_MAX,
    WIFI_PASSWORD_MAX, WIFI_SSID_MAX,
};
pub(crate) use modes::AppStateApplyAck;
pub(crate) use sd::{
    SdCommand, SdCommandKind, SdPowerRequest, SdRequest, SdResult, SdResultCode, SdUploadCommand,
    SdUploadRequest, SdUploadResult, SdUploadResultCode,
};
pub(crate) use serial::SerialStatusEvent;
pub(crate) use trace::TapTraceSample;
pub(crate) use ui_cycle::{UiCycleStepAck, UiCycleStepStatus};
pub(crate) use wall_clock::WallClockQueryResult;
#[cfg(feature = "asset-upload-http")]
pub(crate) use wifi::{
    NetConfigSet, NetControlCommand, WifiConfigRequest, WifiConfigResponse, WifiConfigResultCode,
    WifiCredentials, WifiRuntimePolicy, WIFI_DHCP_TIMEOUT_MAX_MS,
};

#[allow(unused_imports)]
pub(crate) use super::touch::types::{
    Gpio36InputPin, TouchEvent, TouchEventKind, TouchPipelineInput, TouchSampleFrame, TouchStatus,
    TouchSwipeDirection,
};
