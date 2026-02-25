use core::fmt::Write;

use embassy_time::{with_timeout, Duration, Instant};

use crate::firmware::{
    config::SD_RESULTS,
    psram,
    runtime::service_mode,
    touch::debug_log::uart_write_all,
    types::{SdCommand, SdResult, SerialUart, TapTraceSample},
};
#[cfg(feature = "asset-upload-http")]
use crate::firmware::{
    config::{
        WIFI_CONFIG_REQUESTS, WIFI_CONFIG_RESPONSES, WIFI_CONFIG_RESPONSE_TIMEOUT_MS,
        WIFI_CREDENTIALS_UPDATES,
    },
    types::{WifiConfigRequest, WifiCredentials},
};

use super::commands::SdWaitTarget;
#[cfg(feature = "asset-upload-http")]
use super::labels::wifi_config_result_code_label;
use super::labels::{
    sd_command_label, sd_result_code_label, sd_result_kind_label, sdwait_target_label,
};

pub(super) const SD_RESULT_CACHE_CAP: usize = 16;

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

pub(super) async fn run_sdwait_command(
    uart: &mut SerialUart,
    sd_result_cache: &mut heapless::Vec<SdResult, SD_RESULT_CACHE_CAP>,
    last_sd_request_id: Option<u32>,
    target: SdWaitTarget,
    timeout_ms: u32,
) {
    let wait_id = match target {
        SdWaitTarget::Next => None,
        SdWaitTarget::Last => {
            let Some(id) = last_sd_request_id else {
                let _ = uart_write_all(uart, b"SDWAIT ERR reason=no_last_request\r\n").await;
                return;
            };
            Some(id)
        }
        SdWaitTarget::Id(id) => Some(id),
    };

    if let Some(id) = wait_id {
        if let Some(result) = sd_result_cache
            .iter()
            .rev()
            .find(|result| result.id == id)
            .copied()
        {
            write_sdwait_done(uart, target, wait_id, result).await;
            return;
        }
    }

    let start = Instant::now();
    loop {
        let elapsed_ms = Instant::now().saturating_duration_since(start).as_millis();
        if elapsed_ms >= timeout_ms as u64 {
            write_sdwait_timeout(uart, target, wait_id, timeout_ms).await;
            return;
        }

        let remaining_ms = (timeout_ms as u64).saturating_sub(elapsed_ms).max(1);
        match with_timeout(Duration::from_millis(remaining_ms), SD_RESULTS.receive()).await {
            Ok(result) => {
                cache_sd_result(sd_result_cache, result);
                write_sd_result(uart, result).await;
                if wait_id.map(|id| id == result.id).unwrap_or(true) {
                    write_sdwait_done(uart, target, wait_id, result).await;
                    return;
                }
            }
            Err(_) => {
                write_sdwait_timeout(uart, target, wait_id, timeout_ms).await;
                return;
            }
        }
    }
}

async fn write_sdwait_done(
    uart: &mut SerialUart,
    target: SdWaitTarget,
    wait_id: Option<u32>,
    result: SdResult,
) {
    let mut line = heapless::String::<192>::new();
    let _ = write!(
        &mut line,
        "SDWAIT DONE target={} ",
        sdwait_target_label(target)
    );
    if let Some(wait_id) = wait_id {
        let _ = write!(&mut line, "wait_id={} ", wait_id);
    }
    let _ = write!(
        &mut line,
        "id={} op={} status={} code={} attempts={} dur_ms={}\r\n",
        result.id,
        sd_result_kind_label(result.kind),
        if result.ok { "ok" } else { "error" },
        sd_result_code_label(result.code),
        result.attempts,
        result.duration_ms
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

async fn write_sdwait_timeout(
    uart: &mut SerialUart,
    target: SdWaitTarget,
    wait_id: Option<u32>,
    timeout_ms: u32,
) {
    let mut line = heapless::String::<112>::new();
    let _ = write!(
        &mut line,
        "SDWAIT TIMEOUT target={} ",
        sdwait_target_label(target)
    );
    if let Some(wait_id) = wait_id {
        let _ = write!(&mut line, "wait_id={} ", wait_id);
    }
    let _ = write!(&mut line, "timeout_ms={}\r\n", timeout_ms);
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

#[cfg(feature = "asset-upload-http")]
pub(super) async fn run_wifiset_command(uart: &mut SerialUart, credentials: WifiCredentials) {
    while WIFI_CREDENTIALS_UPDATES.try_receive().is_ok() {}
    while WIFI_CONFIG_RESPONSES.try_receive().is_ok() {}

    if WIFI_CREDENTIALS_UPDATES.try_send(credentials).is_err() {
        let _ = uart_write_all(uart, b"WIFISET BUSY\r\n").await;
        return;
    }

    WIFI_CONFIG_REQUESTS
        .send(WifiConfigRequest::Store { credentials })
        .await;

    match with_timeout(
        Duration::from_millis(WIFI_CONFIG_RESPONSE_TIMEOUT_MS),
        WIFI_CONFIG_RESPONSES.receive(),
    )
    .await
    {
        Ok(result) if result.ok => {
            let _ = uart_write_all(uart, b"WIFISET OK\r\n").await;
        }
        Ok(result) => {
            let mut line = heapless::String::<96>::new();
            let _ = write!(
                &mut line,
                "WIFISET ERR reason={}\r\n",
                wifi_config_result_code_label(result.code)
            );
            let _ = uart_write_all(uart, line.as_bytes()).await;
        }
        Err(_) => {
            let _ = uart_write_all(uart, b"WIFISET ERR reason=timeout\r\n").await;
        }
    }
}
