use embassy_futures::select::{select, Either};
use embassy_time::{with_timeout, Duration};
use sdcard::fat::FatEngine;

use super::super::upload::SdUploadSession;
use crate::firmware::config::{SD_REQUESTS, SD_UPLOAD_REQUESTS};
use crate::firmware::types::{SdProbeDriver, SdRequest};

use super::handlers::process_upload_request_and_publish;
#[cfg(not(feature = "asset-upload-http"))]
pub(super) async fn receive_request_without_wifi(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    upload_session: &mut Option<SdUploadSession>,
    fat_engine: &mut FatEngine,
) -> Option<SdRequest> {
    if *powered {
        return receive_request_without_wifi_powered(
            sd_probe,
            powered,
            upload_mounted,
            upload_session,
            fat_engine,
        )
        .await;
    }
    receive_request_without_wifi_unpowered(
        sd_probe,
        powered,
        upload_mounted,
        upload_session,
        fat_engine,
    )
    .await
}

#[cfg(not(feature = "asset-upload-http"))]
async fn receive_request_without_wifi_powered(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    upload_session: &mut Option<SdUploadSession>,
    fat_engine: &mut FatEngine,
) -> Option<SdRequest> {
    match select(
        SD_UPLOAD_REQUESTS.receive(),
        with_timeout(
            Duration::from_millis(super::super::SD_IDLE_POWER_OFF_MS),
            SD_REQUESTS.receive(),
        ),
    )
    .await
    {
        Either::First(upload_request) => {
            process_upload_request_and_publish(
                upload_request,
                upload_session,
                sd_probe,
                powered,
                upload_mounted,
                fat_engine,
            )
            .await;
            None
        }
        Either::Second(result) => result.ok(),
    }
}

#[cfg(not(feature = "asset-upload-http"))]
async fn receive_request_without_wifi_unpowered(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    upload_session: &mut Option<SdUploadSession>,
    fat_engine: &mut FatEngine,
) -> Option<SdRequest> {
    match select(SD_UPLOAD_REQUESTS.receive(), SD_REQUESTS.receive()).await {
        Either::First(upload_request) => {
            process_upload_request_and_publish(
                upload_request,
                upload_session,
                sd_probe,
                powered,
                upload_mounted,
                fat_engine,
            )
            .await;
            None
        }
        Either::Second(request) => Some(request),
    }
}
