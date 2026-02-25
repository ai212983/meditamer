mod app_event;
mod base;
mod modes;
mod sd;
mod time_sync;
mod trace;
#[cfg(feature = "asset-upload-http")]
mod wifi;

pub(crate) use app_event::AppEvent;
pub(crate) use base::{
    DisplayContext, InkplateDriver, PanelPinHold, SdProbeDriver, SerialUart, SD_ASSET_READ_MAX,
    SD_PATH_MAX, SD_UPLOAD_CHUNK_MAX, SD_WRITE_MAX,
};
#[cfg(feature = "asset-upload-http")]
pub(crate) use base::{WIFI_CONFIG_FILE_MAX, WIFI_PASSWORD_MAX, WIFI_SSID_MAX};
pub(crate) use modes::{DisplayMode, RuntimeMode};
pub(crate) use sd::{
    SdAssetReadRequest, SdAssetReadResponse, SdAssetReadResultCode, SdCommand, SdCommandKind,
    SdPowerRequest, SdRequest, SdResult, SdResultCode, SdUploadCommand, SdUploadRequest,
    SdUploadResult, SdUploadResultCode,
};
pub(crate) use time_sync::{TimeSyncCommand, TimeSyncState};
pub(crate) use trace::TapTraceSample;
#[cfg(feature = "asset-upload-http")]
pub(crate) use wifi::{
    WifiConfigRequest, WifiConfigResponse, WifiConfigResultCode, WifiCredentials,
};

#[allow(unused_imports)]
pub(crate) use super::touch::types::{
    TouchEvent, TouchEventKind, TouchIrqPin, TouchPipelineInput, TouchSampleFrame,
    TouchSwipeDirection, TouchTraceSample, TouchWizardSessionEvent, TouchWizardSwipeTraceSample,
};
