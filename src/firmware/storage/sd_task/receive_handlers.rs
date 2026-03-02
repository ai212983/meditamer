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
