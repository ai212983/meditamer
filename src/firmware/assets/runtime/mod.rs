use embassy_time::{with_timeout, Duration};

use super::super::{
    config::{SD_ASSET_READ_REQUESTS, SD_ASSET_READ_RESPONSES},
    types::{SdAssetReadRequest, SdAssetReadResultCode, SD_ASSET_READ_MAX, SD_PATH_MAX},
};

mod pirata;

pub(crate) use pirata::draw_pirata_time_centered;

const SD_ASSET_RESPONSE_TIMEOUT_MS: u64 = 6_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AssetLoadError {
    InvalidPath,
    Timeout,
    Device(SdAssetReadResultCode),
    SizeMismatch,
}

async fn sd_asset_read_roundtrip(
    path: &str,
) -> Result<([u8; SD_ASSET_READ_MAX], usize), AssetLoadError> {
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
        Ok((response.data, response.data_len as usize))
    } else {
        Err(AssetLoadError::Device(response.code))
    }
}
