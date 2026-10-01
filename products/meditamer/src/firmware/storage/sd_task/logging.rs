use super::super::super::{
    config::{SD_DIAG_RESULTS, SD_RESULTS, SD_UPLOAD_RESULTS},
    types::{
        SdCommandKind, SdPowerRequest, SdResult, SdResultCode, SdUploadResult, SdUploadResultCode,
    },
};

pub(super) fn publish_result(result: SdResult) {
    if SD_RESULTS.try_send(result).is_err() {
        console::println!(
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
        console::println!(
            "sdtask: upload_result_drop request_id={} ok={} code={} bytes_written={}",
            result.request_id,
            result.ok as u8,
            sd_upload_result_code_label(result.code),
            result.bytes_written
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

pub(crate) fn sd_result_code_label(code: SdResultCode) -> &'static str {
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

pub(crate) fn now_ms_u32() -> u32 {
    let now_ms = embassy_time::Instant::now().as_millis();
    if now_ms > u32::MAX as u64 {
        u32::MAX
    } else {
        now_ms as u32
    }
}
