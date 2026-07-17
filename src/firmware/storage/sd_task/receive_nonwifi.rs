#[cfg(not(feature = "asset-upload-http"))]
async fn receive_request_without_wifi(
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
                fat_engine,
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
                fat_engine,
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
    fat_engine: &mut FatEngine,
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
                fat_engine,
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
                fat_engine,
            )
            .await;
            None
        }
        Either3::Third(request) => Some(request),
    }
}
