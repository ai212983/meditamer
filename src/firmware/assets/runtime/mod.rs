use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::{with_timeout, Duration};

use super::super::{
    config::{SD_ASSET_READ_REQUESTS, SD_ASSET_READ_RESPONSES},
    runtime::service_mode,
    storage::transfer_buffers,
    types::{SdAssetReadRequest, SdAssetReadResultCode, SD_PATH_MAX},
};

mod pirata;

pub(crate) use pirata::draw_pirata_time_centered;

pub(crate) async fn clear_runtime_asset_caches() {
    pirata::clear_pirata_cache().await;
}

const SD_ASSET_RESPONSE_TIMEOUT_MS: u64 = 6_000;
static SD_ASSET_READ_ROUNDTRIP_LOCK: Mutex<CriticalSectionRawMutex, ()> = Mutex::new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AssetLoadError {
    InvalidPath,
    Disabled,
    Timeout,
    Device(SdAssetReadResultCode),
    SizeMismatch,
}

pub(super) async fn with_sd_asset_read_data<R>(
    path: &str,
    f: impl FnOnce(&[u8]) -> Result<R, AssetLoadError>,
) -> Result<R, AssetLoadError> {
    if !service_mode::asset_reads_enabled() {
        return Err(AssetLoadError::Disabled);
    }
    let _lock = SD_ASSET_READ_ROUNDTRIP_LOCK.lock().await;

    while SD_ASSET_READ_RESPONSES.try_receive().is_ok() {}

    let bytes = path.as_bytes();
    if bytes.is_empty() || bytes.len() > SD_PATH_MAX {
        return Err(AssetLoadError::InvalidPath);
    }

    let mut path_buf = [0u8; SD_PATH_MAX];
    path_buf[..bytes.len()].copy_from_slice(bytes);

    SD_ASSET_READ_REQUESTS
        .send(SdAssetReadRequest {
            path: path_buf,
            path_len: bytes.len() as u8,
        })
        .await;

    let response = match with_timeout(
        Duration::from_millis(SD_ASSET_RESPONSE_TIMEOUT_MS),
        SD_ASSET_READ_RESPONSES.receive(),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => {
            while SD_ASSET_READ_RESPONSES.try_receive().is_ok() {}
            return Err(AssetLoadError::Timeout);
        }
    };

    if response.ok {
        let payload_len = response.data_len as usize;
        let payload = transfer_buffers::lock_asset_read_buffer()
            .await
            .map_err(|_| AssetLoadError::Device(SdAssetReadResultCode::OperationFailed))?;
        if payload_len > payload.as_slice().len() {
            return Err(AssetLoadError::SizeMismatch);
        }
        f(&payload.as_slice()[..payload_len])
    } else {
        Err(AssetLoadError::Device(response.code))
    }
}
