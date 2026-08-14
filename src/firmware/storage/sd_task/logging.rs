#[cfg(feature = "asset-upload-http")]
use super::super::super::{
    config::WIFI_CONFIG_RESPONSES,
    types::{WifiConfigResponse, WifiConfigResultCode},
};
use super::super::super::{
    config::{SD_DIAG_RESULTS, SD_RESULTS, SD_UPLOAD_RESULTS},
    types::{
        SdCommandKind, SdPowerRequest, SdResult, SdResultCode, SdUploadResult, SdUploadResultCode,
    },
};

pub(super) fn publish_result(result: SdResult) {
    if SD_RESULTS.try_send(result).is_err() {
        esp_println::println!(
            "sdtask: result_drop id={} kind={} ok={} code={} attempts={} dur_ms={}",
            result.id,
            sd_kind_label(result.kind),
            result.ok as u8,
            sd_result_code_label(result.code),
            result.attempts,
            result.duration_ms
        );
    }
    let _ = SD_DIAG_RESULTS.try_send(result);
}

pub(super) fn publish_upload_result(mut result: SdUploadResult) {
    if result.chunk_handler_done_at_ms != 0 {
        result.chunk_published_at_ms = now_ms_u32();
        result.chunk_post_handler_ms = result
            .chunk_published_at_ms
            .wrapping_sub(result.chunk_handler_done_at_ms);
    }
    if SD_UPLOAD_RESULTS.try_send(result).is_err() {
        esp_println::println!(
            "sdtask: upload_result_drop request_id={} ok={} code={} bytes_written={}",
            result.request_id,
            result.ok as u8,
            sd_upload_result_code_label(result.code),
            result.bytes_written
        );
    }
}

#[cfg(feature = "asset-upload-http")]
pub(super) fn publish_wifi_config_response(response: WifiConfigResponse) {
    if WIFI_CONFIG_RESPONSES.try_send(response).is_err() {
        esp_println::println!(
            "sdtask: wifi_config_resp_drop id={} ok={} code={} has_credentials={}",
            response.request_id,
            response.ok as u8,
            wifi_config_result_code_label(response.code),
            response.credentials.is_some() as u8
        );
    }
}

pub(super) fn sd_power_action_label(action: SdPowerRequest) -> &'static str {
    match action {
        SdPowerRequest::On => "on",
        SdPowerRequest::Off => "off",
    }
}

fn sd_kind_label(kind: SdCommandKind) -> &'static str {
    match kind {
        SdCommandKind::Probe => "probe",
        SdCommandKind::RwVerify => "rw_verify",
        SdCommandKind::FatList => "fat_ls",
        SdCommandKind::FatRead => "fat_read",
        SdCommandKind::FatWrite => "fat_write",
        SdCommandKind::FatStat => "fat_stat",
        SdCommandKind::FatMkdir => "fat_mkdir",
        SdCommandKind::FatRemove => "fat_rm",
        SdCommandKind::FatRename => "fat_ren",
        SdCommandKind::FatAppend => "fat_append",
        SdCommandKind::FatTruncate => "fat_trunc",
    }
}

pub fn sd_result_code_label(code: SdResultCode) -> &'static str {
    match code {
        SdResultCode::Ok => "ok",
        SdResultCode::PowerOnFailed => "power_on_failed",
        SdResultCode::InitFailed => "init_failed",
        SdResultCode::InvalidPath => "invalid_path",
        SdResultCode::NotFound => "not_found",
        SdResultCode::VerifyMismatch => "verify_mismatch",
        SdResultCode::PowerOffFailed => "power_off_failed",
        SdResultCode::OperationFailed => "operation_failed",
        SdResultCode::RefusedLba0 => "refused_lba0",
    }
}

fn sd_upload_result_code_label(code: SdUploadResultCode) -> &'static str {
    match code {
        SdUploadResultCode::Ok => "ok",
        SdUploadResultCode::Busy => "busy",
        SdUploadResultCode::SessionNotActive => "session_not_active",
        SdUploadResultCode::InvalidPath => "invalid_path",
        SdUploadResultCode::NotFound => "not_found",
        SdUploadResultCode::NotEmpty => "not_empty",
        SdUploadResultCode::SizeMismatch => "size_mismatch",
        SdUploadResultCode::PowerOnFailed => "power_on_failed",
        SdUploadResultCode::InitFailed => "init_failed",
        SdUploadResultCode::DirectoryFull => "directory_full",
        SdUploadResultCode::OperationFailed => "operation_failed",
    }
}

pub fn now_ms_u32() -> u32 {
    let now_ms = embassy_time::Instant::now().as_millis();
    if now_ms > u32::MAX as u64 {
        u32::MAX
    } else {
        now_ms as u32
    }
}

#[cfg(feature = "asset-upload-http")]
fn wifi_config_result_code_label(code: WifiConfigResultCode) -> &'static str {
    match code {
        WifiConfigResultCode::Ok => "ok",
        WifiConfigResultCode::Busy => "busy",
        WifiConfigResultCode::NotFound => "not_found",
        WifiConfigResultCode::InvalidData => "invalid_data",
        WifiConfigResultCode::PowerOnFailed => "power_on_failed",
        WifiConfigResultCode::InitFailed => "init_failed",
        WifiConfigResultCode::OperationFailed => "operation_failed",
    }
}
