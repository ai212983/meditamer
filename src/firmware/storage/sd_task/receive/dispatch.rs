#[cfg(not(feature = "asset-upload-http"))]
use super::nonwifi::receive_request_without_wifi;
#[cfg(feature = "asset-upload-http")]
use super::wifi::receive_request_with_wifi;
#[cfg(feature = "asset-upload-http")]
use embassy_futures::select::{select3, Either3};
use sdcard::fat::FatEngine;

#[cfg(feature = "asset-upload-http")]
use super::super::super::super::config::WIFI_CONFIG_REQUESTS;
use super::super::super::super::types::{SdProbeDriver, SdRequest};
#[cfg(feature = "asset-upload-http")]
use super::super::logging::publish_wifi_config_response;
use super::super::upload::SdUploadSession;
#[cfg(feature = "asset-upload-http")]
use super::super::wifi_config::process_wifi_config_request;

pub(in crate::firmware::storage::sd_task) async fn receive_core_request(
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
