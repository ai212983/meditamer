#[cfg(not(feature = "asset-upload-http"))]
use embassy_futures::select::{select, Either};
#[cfg(feature = "asset-upload-http")]
use embassy_futures::select::{select3, Either3};
use embassy_time::{with_timeout, Duration};
use sdcard::fat::FatEngine;

#[cfg(feature = "asset-upload-http")]
use super::super::super::config::WIFI_CONFIG_REQUESTS;
use super::super::super::{
    config::{SD_REQUESTS, SD_UPLOAD_REQUESTS},
    types::{SdProbeDriver, SdRequest},
};
use super::logging::publish_upload_result;
#[cfg(feature = "asset-upload-http")]
use super::logging::publish_wifi_config_response;
use super::upload::{process_upload_request, SdUploadSession};
#[cfg(feature = "asset-upload-http")]
use super::wifi_config::process_wifi_config_request;

pub(super) async fn receive_core_request(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    upload_session: &mut Option<SdUploadSession>,
    fat_engine: &mut FatEngine,
) -> Option<SdRequest> {
    loop {
        #[cfg(feature = "asset-upload-http")]
        {
            if let Some(request) = receive_request_with_wifi(
                sd_probe,
                powered,
                upload_mounted,
                upload_session,
                fat_engine,
            )
            .await
            {
                return Some(request);
            }
        }

        #[cfg(not(feature = "asset-upload-http"))]
        {
            if let Some(request) = receive_request_without_wifi(
                sd_probe,
                powered,
                upload_mounted,
                upload_session,
                fat_engine,
            )
            .await
            {
                return Some(request);
            }
        }
    }
}
