use core::sync::atomic::Ordering;

use super::super::counters::*;
#[cfg(feature = "asset-upload-http")]
use super::super::types::SdUploadRoundtripPhase;
#[cfg(all(feature = "asset-upload-http", feature = "telemetry-defmt"))]
use super::helpers::sd_upload_result_code_to_u8;
#[cfg(feature = "asset-upload-http")]
use super::helpers::{saturating_add_u32, update_max_u32};
#[cfg(feature = "asset-upload-http")]
use crate::firmware::types::SdUploadResultCode;

#[cfg(feature = "asset-upload-http")]
pub(crate) fn record_sd_upload_roundtrip_timeout() {
    SD_UPLOAD_ERRORS.fetch_add(1, Ordering::Relaxed);
    SD_UPLOAD_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry sd_upload_roundtrip_timeout");
}

#[cfg(feature = "asset-upload-http")]
pub(crate) fn record_sd_upload_roundtrip_code(code: SdUploadResultCode) {
    SD_UPLOAD_ERRORS.fetch_add(1, Ordering::Relaxed);
    match code {
        SdUploadResultCode::Busy => {
            SD_UPLOAD_BUSY.fetch_add(1, Ordering::Relaxed);
        }
        SdUploadResultCode::PowerOnFailed => {
            SD_UPLOAD_POWER_ON_FAILED.fetch_add(1, Ordering::Relaxed);
        }
        SdUploadResultCode::InitFailed => {
            SD_UPLOAD_INIT_FAILED.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!(
        "telemetry sd_upload_roundtrip_code code={=u8}",
        sd_upload_result_code_to_u8(code),
    );
}

#[cfg(feature = "asset-upload-http")]
pub(crate) fn record_sd_upload_roundtrip_timing(phase: SdUploadRoundtripPhase, elapsed_ms: u32) {
    let (count, total, max) = match phase {
        SdUploadRoundtripPhase::Begin => (
            &SD_UPLOAD_RTT_BEGIN_COUNT,
            &SD_UPLOAD_RTT_BEGIN_MS_TOTAL,
            &SD_UPLOAD_RTT_BEGIN_MS_MAX,
        ),
        SdUploadRoundtripPhase::Chunk => (
            &SD_UPLOAD_RTT_CHUNK_COUNT,
            &SD_UPLOAD_RTT_CHUNK_MS_TOTAL,
            &SD_UPLOAD_RTT_CHUNK_MS_MAX,
        ),
        SdUploadRoundtripPhase::Commit => (
            &SD_UPLOAD_RTT_COMMIT_COUNT,
            &SD_UPLOAD_RTT_COMMIT_MS_TOTAL,
            &SD_UPLOAD_RTT_COMMIT_MS_MAX,
        ),
        SdUploadRoundtripPhase::Abort => (
            &SD_UPLOAD_RTT_ABORT_COUNT,
            &SD_UPLOAD_RTT_ABORT_MS_TOTAL,
            &SD_UPLOAD_RTT_ABORT_MS_MAX,
        ),
        SdUploadRoundtripPhase::Mkdir => (
            &SD_UPLOAD_RTT_MKDIR_COUNT,
            &SD_UPLOAD_RTT_MKDIR_MS_TOTAL,
            &SD_UPLOAD_RTT_MKDIR_MS_MAX,
        ),
        SdUploadRoundtripPhase::Remove => (
            &SD_UPLOAD_RTT_REMOVE_COUNT,
            &SD_UPLOAD_RTT_REMOVE_MS_TOTAL,
            &SD_UPLOAD_RTT_REMOVE_MS_MAX,
        ),
    };
    count.fetch_add(1, Ordering::Relaxed);
    saturating_add_u32(total, elapsed_ms);
    update_max_u32(max, elapsed_ms);
}

pub(crate) fn set_boot_reset_reason_code(code: Option<u8>) {
    BOOT_RESET_REASON_CODE.store(code.unwrap_or(0) as u32, Ordering::Relaxed);
}

#[cfg(feature = "asset-upload-http")]
pub(crate) fn record_sd_upload_session_timeout_abort() {
    SD_UPLOAD_SESSION_TIMEOUT_ABORTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry sd_upload_session_timeout_abort");
}

#[cfg(feature = "asset-upload-http")]
pub(crate) fn record_sd_upload_session_mode_off_abort() {
    SD_UPLOAD_SESSION_MODE_OFF_ABORTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry sd_upload_session_mode_off_abort");
}
