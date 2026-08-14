#[cfg(feature = "asset-upload-http")]
use sdcard::fat::FatEngine;

#[cfg(feature = "asset-upload-http")]
use super::super::super::types::{SdPowerRequest, SdProbeDriver};
#[cfg(feature = "asset-upload-http")]
use super::power::request_sd_power;

#[cfg(feature = "asset-upload-http")]
use super::super::super::types::{
    SdUploadCommand, SdUploadRequest, SdUploadResult, SdUploadResultCode, WifiConfigResponse,
    WifiConfigResultCode,
};
#[cfg(feature = "asset-upload-http")]
use super::upload::SdUploadSession;

#[cfg(feature = "asset-upload-http")]
use super::upload::process_upload_request;
#[cfg(feature = "asset-upload-http")]
pub(super) fn disabled_upload_result(request_id: u32) -> SdUploadResult {
    SdUploadResult {
        request_id,
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
pub(super) fn disabled_wifi_config_response(request_id: u32) -> WifiConfigResponse {
    WifiConfigResponse {
        request_id,
        ok: false,
        code: WifiConfigResultCode::Busy,
        credentials: None,
    }
}

#[cfg(feature = "asset-upload-http")]
pub(super) fn wifi_config_error_response(
    request_id: u32,
    code: SdUploadResultCode,
) -> WifiConfigResponse {
    let mapped = match code {
        SdUploadResultCode::PowerOnFailed => WifiConfigResultCode::PowerOnFailed,
        SdUploadResultCode::InitFailed => WifiConfigResultCode::InitFailed,
        _ => WifiConfigResultCode::OperationFailed,
    };
    WifiConfigResponse {
        request_id,
        ok: false,
        code: mapped,
        credentials: None,
    }
}

#[cfg(feature = "asset-upload-http")]
pub(super) async fn ensure_upload_storage_ready(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) -> Result<(), SdUploadResultCode> {
    if !*powered {
        if !request_sd_power(SdPowerRequest::On).await {
            return Err(SdUploadResultCode::PowerOnFailed);
        }
        *powered = true;
        *upload_mounted = false;
    }

    if !*upload_mounted {
        if !sd_probe.is_initialized() {
            // SdCardProbe owns the overall cooperative deadline. Do not wrap
            // initialization in a timeout that can drop an in-flight DMA
            // transfer and poison the shared ESP32 SPI/DMA bus.
            match sd_probe.init().await {
                Ok(_) => {}
                Err(_) => {
                    sd_probe.recover_after_timeout();
                    fat_engine.invalidate();
                    *upload_mounted = false;
                    return Err(SdUploadResultCode::InitFailed);
                }
            }
        }
        *upload_mounted = true;
    }

    Ok(())
}

#[cfg(feature = "asset-upload-http")]
pub(super) async fn abort_active_upload_session(
    upload_session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) -> SdUploadResult {
    process_upload_request(
        SdUploadRequest {
            id: 0,
            command: SdUploadCommand::Abort,
            enqueued_at_ms: now_ms_u32(),
        },
        upload_session,
        sd_probe,
        powered,
        upload_mounted,
        fat_engine,
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
