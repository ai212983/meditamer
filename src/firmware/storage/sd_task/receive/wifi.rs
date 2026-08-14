#[cfg(feature = "asset-upload-http")]
pub(super) async fn receive_request_with_wifi(
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
            Duration::from_millis(super::super::SD_IDLE_POWER_OFF_MS),
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
