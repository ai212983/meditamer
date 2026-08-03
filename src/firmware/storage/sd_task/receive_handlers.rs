async fn process_upload_request_and_publish(
    upload_request: crate::firmware::types::SdUploadRequest,
    upload_session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) {
    let result = process_upload_request(
        upload_request,
        upload_session,
        sd_probe,
        powered,
        upload_mounted,
        fat_engine,
    )
    .await;
    publish_upload_result(result);
}
