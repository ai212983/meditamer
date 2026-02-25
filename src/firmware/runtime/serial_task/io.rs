use core::fmt::Write;

use super::labels::{sd_command_label, sd_result_code_label, sd_result_kind_label};
use crate::firmware::{
    psram,
    runtime::service_mode,
    touch::debug_log::uart_write_all,
    types::{SdCommand, SdResult, SerialUart, TapTraceSample},
};

mod sdwait;
#[cfg(feature = "asset-upload-http")]
mod wifiset;

pub(super) const SD_RESULT_CACHE_CAP: usize = 16;
pub(super) use sdwait::run_sdwait_command;
#[cfg(feature = "asset-upload-http")]
pub(super) use wifiset::run_wifiset_command;

pub(super) async fn write_tap_trace_sample(uart: &mut SerialUart, sample: TapTraceSample) {
    let mut line = heapless::String::<256>::new();
    let _ = write!(
        &mut line,
        "tap_trace,{},{:#04x},{},{},{:#04x},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\r\n",
        sample.t_ms,
        sample.tap_src,
        sample.seq_count,
        sample.tap_candidate,
        sample.cand_src,
        sample.state_id,
        sample.reject_reason,
        sample.candidate_score,
        sample.window_ms,
        sample.cooldown_active,
        sample.jerk_l1,
        sample.motion_veto,
        sample.gyro_l1,
        sample.int1,
        sample.int2,
        sample.power_good,
        sample.battery_percent,
        sample.gx,
        sample.gy,
        sample.gz,
        sample.ax,
        sample.ay,
        sample.az
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

pub(super) async fn write_allocator_status_line(uart: &mut SerialUart) {
    let status = psram::allocator_status();
    let used_bytes = status.total_bytes.saturating_sub(status.free_bytes);
    let mut line = heapless::String::<192>::new();
    let _ = write!(
        &mut line,
        "PSRAM feature_enabled={} state={:?} total_bytes={} used_bytes={} free_bytes={} peak_used_bytes={}\r\n",
        status.feature_enabled,
        status.state,
        status.total_bytes,
        used_bytes,
        status.free_bytes,
        status.peak_used_bytes
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

pub(super) async fn write_mode_status_line(uart: &mut SerialUart) {
    let services = service_mode::runtime_services();
    let mut line = heapless::String::<128>::new();
    let _ = write!(
        &mut line,
        "MODE upload={} assets={}\r\n",
        if services.upload_enabled_flag() {
            "on"
        } else {
            "off"
        },
        if services.asset_reads_enabled_flag() {
            "on"
        } else {
            "off"
        }
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

pub(super) async fn run_allocator_alloc_probe(uart: &mut SerialUart, bytes: usize) {
    match psram::alloc_large_byte_buffer(bytes) {
        Ok(mut buffer) => {
            #[cfg(not(feature = "psram-alloc"))]
            let _ = &mut buffer;

            #[cfg(feature = "psram-alloc")]
            if let Some(first) = buffer.as_mut_slice().first_mut() {
                *first = 0xA5;
            }

            let mut line = heapless::String::<128>::new();
            let _ = write!(
                &mut line,
                "PSRAMALLOC OK bytes={} placement={:?} len={}\r\n",
                bytes,
                buffer.placement(),
                buffer.len()
            );
            let _ = uart_write_all(uart, line.as_bytes()).await;
            psram::log_allocator_high_water("serial_psram_alloc_probe");
        }
        Err(err) => {
            let mut line = heapless::String::<128>::new();
            let _ = write!(
                &mut line,
                "PSRAMALLOC ERR bytes={} reason={:?}\r\n",
                bytes, err
            );
            let _ = uart_write_all(uart, line.as_bytes()).await;
        }
    }
}

pub(super) async fn write_sd_request_queued(
    uart: &mut SerialUart,
    request_id: u32,
    command: SdCommand,
) {
    let mut line = heapless::String::<96>::new();
    let _ = write!(
        &mut line,
        "SDREQ id={} op={}\r\n",
        request_id,
        sd_command_label(command)
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

pub(super) async fn write_sd_result(uart: &mut SerialUart, result: SdResult) {
    let mut line = heapless::String::<128>::new();
    let _ = write!(
        &mut line,
        "SDDONE id={} op={} status={} code={} attempts={} dur_ms={}\r\n",
        result.id,
        sd_result_kind_label(result.kind),
        if result.ok { "ok" } else { "error" },
        sd_result_code_label(result.code),
        result.attempts,
        result.duration_ms
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

pub(super) fn cache_sd_result(
    cache: &mut heapless::Vec<SdResult, SD_RESULT_CACHE_CAP>,
    result: SdResult,
) {
    if cache.push(result).is_err() {
        let _ = cache.remove(0);
        let _ = cache.push(result);
    }
}
