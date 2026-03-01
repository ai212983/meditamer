use embassy_futures::select::{select3, Either3};
use embassy_time::{with_timeout, Duration};

#[cfg(not(feature = "asset-upload-http"))]
use super::super::super::config::SD_ASSET_READ_REQUESTS;
#[cfg(feature = "asset-upload-http")]
use super::super::super::config::WIFI_CONFIG_REQUESTS;
use super::super::super::{
    config::{SD_REQUESTS, SD_UPLOAD_REQUESTS},
    types::{SdProbeDriver, SdRequest},
};
#[cfg(not(feature = "asset-upload-http"))]
use super::asset_read::process_asset_read_request;
#[cfg(not(feature = "asset-upload-http"))]
use super::logging::publish_asset_read_response;
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
) -> Option<SdRequest> {
    loop {
        #[cfg(feature = "asset-upload-http")]
        {
            if let Some(request) =
                receive_request_with_wifi(sd_probe, powered, upload_mounted, upload_session).await
            {
                return Some(request);
            }
        }

        #[cfg(not(feature = "asset-upload-http"))]
        {
            if let Some(request) =
                receive_request_without_wifi(sd_probe, powered, upload_mounted, upload_session)
                    .await
            {
                return Some(request);
            }
        }
    }
}

#[cfg(feature = "asset-upload-http")]
async fn receive_request_with_wifi(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    upload_session: &mut Option<SdUploadSession>,
) -> Option<SdRequest> {
    if *powered {
        return receive_request_with_wifi_powered(
            sd_probe,
            powered,
            upload_mounted,
            upload_session,
        )
        .await;
    }
    receive_request_with_wifi_unpowered(sd_probe, powered, upload_mounted, upload_session).await
}

#[cfg(feature = "asset-upload-http")]
async fn receive_request_with_wifi_powered(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    upload_session: &mut Option<SdUploadSession>,
) -> Option<SdRequest> {
    match select3(
        WIFI_CONFIG_REQUESTS.receive(),
        SD_UPLOAD_REQUESTS.receive(),
        with_timeout(
            Duration::from_millis(super::SD_IDLE_POWER_OFF_MS),
            SD_REQUESTS.receive(),
        ),
    )
    .await
    {
        Either3::First(config_request) => {
            process_wifi_request(
                config_request,
                upload_session,
                sd_probe,
                powered,
                upload_mounted,
            )
            .await;
            None
        }
        Either3::Second(upload_request) => {
            process_upload_request_and_publish(
                upload_request,
                upload_session,
                sd_probe,
                powered,
                upload_mounted,
            )
            .await;
            None
        }
        Either3::Third(result) => result.ok(),
    }
}

#[cfg(feature = "asset-upload-http")]
async fn receive_request_with_wifi_unpowered(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    upload_session: &mut Option<SdUploadSession>,
) -> Option<SdRequest> {
    match select3(
        WIFI_CONFIG_REQUESTS.receive(),
        SD_UPLOAD_REQUESTS.receive(),
        SD_REQUESTS.receive(),
    )
    .await
    {
        Either3::First(config_request) => {
            process_wifi_request(
                config_request,
                upload_session,
                sd_probe,
                powered,
                upload_mounted,
            )
            .await;
            None
        }
        Either3::Second(upload_request) => {
            process_upload_request_and_publish(
                upload_request,
                upload_session,
                sd_probe,
                powered,
                upload_mounted,
            )
            .await;
            None
        }
        Either3::Third(request) => Some(request),
    }
}

#[cfg(feature = "asset-upload-http")]
async fn process_wifi_request(
    config_request: crate::firmware::types::WifiConfigRequest,
    upload_session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) {
    let response = process_wifi_config_request(
        config_request,
        upload_session,
        sd_probe,
        powered,
        upload_mounted,
    )
    .await;
    publish_wifi_config_response(response);
}

#[cfg(not(feature = "asset-upload-http"))]
async fn receive_request_without_wifi(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    upload_session: &mut Option<SdUploadSession>,
) -> Option<SdRequest> {
    if *powered {
        return receive_request_without_wifi_powered(
            sd_probe,
            powered,
            upload_mounted,
            upload_session,
        )
        .await;
    }
    receive_request_without_wifi_unpowered(sd_probe, powered, upload_mounted, upload_session).await
}

#[cfg(not(feature = "asset-upload-http"))]
async fn receive_request_without_wifi_powered(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    upload_session: &mut Option<SdUploadSession>,
) -> Option<SdRequest> {
    match select3(
        SD_UPLOAD_REQUESTS.receive(),
        SD_ASSET_READ_REQUESTS.receive(),
        with_timeout(
            Duration::from_millis(super::SD_IDLE_POWER_OFF_MS),
            SD_REQUESTS.receive(),
        ),
    )
    .await
    {
        Either3::First(upload_request) => {
            process_upload_request_and_publish(
                upload_request,
                upload_session,
                sd_probe,
                powered,
                upload_mounted,
            )
            .await;
            None
        }
        Either3::Second(asset_request) => {
            process_asset_read_request_and_publish(
                asset_request,
                upload_session,
                sd_probe,
                powered,
                upload_mounted,
            )
            .await;
            None
        }
        Either3::Third(result) => result.ok(),
    }
}

#[cfg(not(feature = "asset-upload-http"))]
async fn receive_request_without_wifi_unpowered(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    upload_session: &mut Option<SdUploadSession>,
) -> Option<SdRequest> {
    match select3(
        SD_UPLOAD_REQUESTS.receive(),
        SD_ASSET_READ_REQUESTS.receive(),
        SD_REQUESTS.receive(),
    )
    .await
    {
        Either3::First(upload_request) => {
            process_upload_request_and_publish(
                upload_request,
                upload_session,
                sd_probe,
                powered,
                upload_mounted,
            )
            .await;
            None
        }
        Either3::Second(asset_request) => {
            process_asset_read_request_and_publish(
                asset_request,
                upload_session,
                sd_probe,
                powered,
                upload_mounted,
            )
            .await;
            None
        }
        Either3::Third(request) => Some(request),
    }
}

async fn process_upload_request_and_publish(
    upload_request: crate::firmware::types::SdUploadRequest,
    upload_session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) {
    let result = process_upload_request(
        upload_request,
        upload_session,
        sd_probe,
        powered,
        upload_mounted,
    )
    .await;
    publish_upload_result(result);
}

#[cfg(not(feature = "asset-upload-http"))]
async fn process_asset_read_request_and_publish(
    asset_request: crate::firmware::types::SdAssetReadRequest,
    upload_session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) {
    let response = process_asset_read_request(
        asset_request,
        upload_session,
        sd_probe,
        powered,
        upload_mounted,
    )
    .await;
    publish_asset_read_response(response);
}
