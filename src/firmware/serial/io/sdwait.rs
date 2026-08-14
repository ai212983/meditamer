use core::fmt::Write;

use embassy_time::{with_timeout, Duration, Instant};

use crate::firmware::{
    config::SD_RESULTS,
    touch::debug_log::uart_write_all,
    types::{SdResult, SerialUart},
};

use super::super::{commands::SdWaitTarget, labels::sdwait_target_label};
use super::{cache_sd_result, write_sd_result, SD_RESULT_CACHE_CAP};

pub(crate) async fn run_sdwait_command(
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
        super::super::labels::sd_result_kind_label(result.kind),
        if result.ok { "ok" } else { "error" },
        super::super::labels::sd_result_code_label(result.code),
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
