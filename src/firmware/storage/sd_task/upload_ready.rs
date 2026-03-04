#[cfg(feature = "asset-upload-http")]
fn disabled_upload_result() -> SdUploadResult {
    SdUploadResult {
        ok: false,
        code: SdUploadResultCode::Busy,
        bytes_written: 0,
        chunk_queue_wait_ms: 0,
        chunk_handler_ms: 0,
        chunk_post_handler_ms: 0,
        chunk_published_at_ms: 0,
        chunk_handler_done_at_ms: 0,
    }
}

#[cfg(feature = "asset-upload-http")]
fn disabled_wifi_config_response() -> WifiConfigResponse {
    WifiConfigResponse {
        ok: false,
        code: WifiConfigResultCode::Busy,
        credentials: None,
    }
}

#[cfg(feature = "asset-upload-http")]
fn wifi_config_error_response(code: SdUploadResultCode) -> WifiConfigResponse {
    let mapped = match code {
        SdUploadResultCode::PowerOnFailed => WifiConfigResultCode::PowerOnFailed,
        SdUploadResultCode::InitFailed => WifiConfigResultCode::InitFailed,
        _ => WifiConfigResultCode::OperationFailed,
    };
    WifiConfigResponse {
        ok: false,
        code: mapped,
        credentials: None,
    }
}

#[cfg(feature = "asset-upload-http")]
async fn ensure_upload_storage_ready(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> Result<(), SdUploadResultCode> {
    if !*powered {
        if !request_sd_power(SdPowerRequest::On).await {
            return Err(SdUploadResultCode::PowerOnFailed);
        }
        *powered = true;
        *upload_mounted = false;
    }

    if !*upload_mounted {
        if !sd_probe.is_initialized() && sd_probe.init().await.is_err() {
            return Err(SdUploadResultCode::InitFailed);
        }
        *upload_mounted = true;
    }

    Ok(())
}

#[cfg(feature = "asset-upload-http")]
async fn abort_active_upload_session(
    upload_session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdUploadResult {
    process_upload_request(
        SdUploadRequest {
            command: SdUploadCommand::Abort,
            enqueued_at_ms: now_ms_u32(),
        },
        upload_session,
        sd_probe,
        powered,
        upload_mounted,
    )
    .await
}

#[cfg(feature = "asset-upload-http")]
fn now_ms_u32() -> u32 {
    let now_ms = embassy_time::Instant::now().as_millis();
    if now_ms > u32::MAX as u64 {
        u32::MAX
    } else {
        now_ms as u32
    }
}
