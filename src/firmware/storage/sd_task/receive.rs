#[cfg(not(feature = "asset-upload-http"))]
use embassy_futures::select::{select3, Either3};
#[cfg(feature = "asset-upload-http")]
use embassy_futures::select::{select4, Either4};
use embassy_time::{with_timeout, Duration};

#[cfg(feature = "asset-upload-http")]
use super::super::super::config::WIFI_CONFIG_REQUESTS;
use super::super::super::{
    config::{SD_ASSET_READ_REQUESTS, SD_REQUESTS, SD_UPLOAD_REQUESTS},
    types::{SdProbeDriver, SdRequest},
};
use super::asset_read::process_asset_read_request;
#[cfg(feature = "asset-upload-http")]
use super::logging::publish_wifi_config_response;
use super::logging::{publish_asset_read_response, publish_upload_result};
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
            if *powered {
                match select4(
                    WIFI_CONFIG_REQUESTS.receive(),
                    SD_UPLOAD_REQUESTS.receive(),
                    SD_ASSET_READ_REQUESTS.receive(),
                    with_timeout(
                        Duration::from_millis(super::SD_IDLE_POWER_OFF_MS),
                        SD_REQUESTS.receive(),
                    ),
                )
                .await
                {
                    Either4::First(config_request) => {
                        let response = process_wifi_config_request(
                            config_request,
                            upload_session,
                            sd_probe,
                            powered,
                            upload_mounted,
                        )
                        .await;
                        publish_wifi_config_response(response);
                        continue;
                    }
                    Either4::Second(upload_request) => {
                        let result = process_upload_request(
                            upload_request,
                            upload_session,
                            sd_probe,
                            powered,
                            upload_mounted,
                        )
                        .await;
                        publish_upload_result(result);
                        continue;
                    }
                    Either4::Third(asset_request) => {
                        let response = process_asset_read_request(
                            asset_request,
                            upload_session,
                            sd_probe,
                            powered,
                            upload_mounted,
                        )
                        .await;
                        publish_asset_read_response(response);
                        continue;
                    }
                    Either4::Fourth(result) => return result.ok(),
                }
            } else {
                match select4(
                    WIFI_CONFIG_REQUESTS.receive(),
                    SD_UPLOAD_REQUESTS.receive(),
                    SD_ASSET_READ_REQUESTS.receive(),
                    SD_REQUESTS.receive(),
                )
                .await
                {
                    Either4::First(config_request) => {
                        let response = process_wifi_config_request(
                            config_request,
                            upload_session,
                            sd_probe,
                            powered,
                            upload_mounted,
                        )
                        .await;
                        publish_wifi_config_response(response);
                        continue;
                    }
                    Either4::Second(upload_request) => {
                        let result = process_upload_request(
                            upload_request,
                            upload_session,
                            sd_probe,
                            powered,
                            upload_mounted,
                        )
                        .await;
                        publish_upload_result(result);
                        continue;
                    }
                    Either4::Third(asset_request) => {
                        let response = process_asset_read_request(
                            asset_request,
                            upload_session,
                            sd_probe,
                            powered,
                            upload_mounted,
                        )
                        .await;
                        publish_asset_read_response(response);
                        continue;
                    }
                    Either4::Fourth(request) => return Some(request),
                }
            }
        }

        #[cfg(not(feature = "asset-upload-http"))]
        {
            if *powered {
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
                        let result = process_upload_request(
                            upload_request,
                            upload_session,
                            sd_probe,
                            powered,
                            upload_mounted,
                        )
                        .await;
                        publish_upload_result(result);
                        continue;
                    }
                    Either3::Second(asset_request) => {
                        let response = process_asset_read_request(
                            asset_request,
                            upload_session,
                            sd_probe,
                            powered,
                            upload_mounted,
                        )
                        .await;
                        publish_asset_read_response(response);
                        continue;
                    }
                    Either3::Third(result) => return result.ok(),
                }
            } else {
                match select3(
                    SD_UPLOAD_REQUESTS.receive(),
                    SD_ASSET_READ_REQUESTS.receive(),
                    SD_REQUESTS.receive(),
                )
                .await
                {
                    Either3::First(upload_request) => {
                        let result = process_upload_request(
                            upload_request,
                            upload_session,
                            sd_probe,
                            powered,
                            upload_mounted,
                        )
                        .await;
                        publish_upload_result(result);
                        continue;
                    }
                    Either3::Second(asset_request) => {
                        let response = process_asset_read_request(
                            asset_request,
                            upload_session,
                            sd_probe,
                            powered,
                            upload_mounted,
                        )
                        .await;
                        publish_asset_read_response(response);
                        continue;
                    }
                    Either3::Third(request) => return Some(request),
                }
            }
        }
    }
}
