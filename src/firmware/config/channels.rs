use core::sync::atomic::AtomicU32;

use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, mutex::Mutex};

#[cfg(feature = "asset-upload-http")]
use super::super::types::WifiCredentials;
use super::super::types::{
    AppEvent, SdAssetReadRequest, SdAssetReadResponse, SdPowerRequest, SdRequest, SdResult,
    SdUploadRequest, SdUploadResult, TapTraceSample, SD_ASSET_READ_MAX, SD_UPLOAD_CHUNK_MAX,
};
#[cfg(feature = "asset-upload-http")]
use super::super::types::{WifiConfigRequest, WifiConfigResponse};

pub(crate) static APP_EVENTS: Channel<CriticalSectionRawMutex, AppEvent, 8> = Channel::new();
pub(crate) static SD_REQUESTS: Channel<CriticalSectionRawMutex, SdRequest, 8> = Channel::new();
pub(crate) static SD_RESULTS: Channel<CriticalSectionRawMutex, SdResult, 16> = Channel::new();
pub(crate) static SD_UPLOAD_REQUESTS: Channel<CriticalSectionRawMutex, SdUploadRequest, 2> =
    Channel::new();
pub(crate) static SD_UPLOAD_RESULTS: Channel<CriticalSectionRawMutex, SdUploadResult, 2> =
    Channel::new();
pub(crate) static SD_ASSET_READ_REQUESTS: Channel<CriticalSectionRawMutex, SdAssetReadRequest, 2> =
    Channel::new();
pub(crate) static SD_ASSET_READ_RESPONSES: Channel<
    CriticalSectionRawMutex,
    SdAssetReadResponse,
    1,
> = Channel::new();
pub(crate) static SD_ASSET_READ_BUFFER: Mutex<CriticalSectionRawMutex, [u8; SD_ASSET_READ_MAX]> =
    Mutex::new([0; SD_ASSET_READ_MAX]);
pub(crate) static SD_UPLOAD_CHUNK_BUFFER: Mutex<
    CriticalSectionRawMutex,
    [u8; SD_UPLOAD_CHUNK_MAX],
> = Mutex::new([0; SD_UPLOAD_CHUNK_MAX]);
#[cfg(feature = "asset-upload-http")]
pub(crate) static WIFI_CREDENTIALS_UPDATES: Channel<CriticalSectionRawMutex, WifiCredentials, 2> =
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
pub(crate) static TAP_TRACE_SAMPLES: Channel<CriticalSectionRawMutex, TapTraceSample, 8> =
    Channel::new();
pub(crate) static LAST_MARBLE_REDRAW_MS: AtomicU32 = AtomicU32::new(0);
pub(crate) static MAX_MARBLE_REDRAW_MS: AtomicU32 = AtomicU32::new(0);
