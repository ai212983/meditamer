use core::fmt::Write;

use super::labels::{sd_command_label, sd_result_code_label, sd_result_kind_label};
use crate::firmware::{
    app_state::{
        read_app_state_snapshot, BaseMode, DayBackground, DiagKind, DiagTargets, OverlayMode,
    },
    psram,
    runtime::diagnostics::read_diag_runtime_status,
    touch::debug_log::uart_write_all,
    types::{SdCommand, SdResult, SerialUart, TapTraceSample},
};

#[cfg(feature = "asset-upload-http")]
mod netcfg;
mod sdwait;
mod state_ack;

pub(super) const SD_RESULT_CACHE_CAP: usize = 16;
#[cfg(feature = "asset-upload-http")]
pub(super) use netcfg::{run_netcfg_get_command, run_netcfg_set_command};
pub(super) use sdwait::run_sdwait_command;
pub(super) use state_ack::{drain_app_state_apply_acks, wait_app_state_apply_ack};

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
    let snapshot = psram::allocator_memory_snapshot();
    let mut line = heapless::String::<320>::new();
    let _ = write!(
        &mut line,
        "PSRAM feature_enabled={} state={:?} total_bytes={} used_bytes={} free_bytes={} peak_used_bytes={} internal_free_bytes={} external_free_bytes={} min_free_bytes={} min_internal_free_bytes={} min_external_free_bytes={}\r\n",
        snapshot.feature_enabled,
        snapshot.state,
        snapshot.total_bytes,
        snapshot.used_bytes,
        snapshot.free_bytes,
        snapshot.peak_used_bytes,
        snapshot.free_internal_bytes,
        snapshot.free_external_bytes,
        snapshot.min_free_bytes,
        snapshot.min_free_internal_bytes,
        snapshot.min_free_external_bytes
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

fn phase_label(phase: crate::firmware::app_state::Phase) -> &'static str {
    match phase {
        crate::firmware::app_state::Phase::Initializing => "INITIALIZING",
        crate::firmware::app_state::Phase::Operating => "OPERATING",
        crate::firmware::app_state::Phase::DiagnosticsExclusive => "DIAGNOSTICS_EXCLUSIVE",
    }
}

fn base_label(base: BaseMode) -> &'static str {
    match base {
        BaseMode::Day => "DAY",
        BaseMode::TouchWizard => "TOUCH_WIZARD",
    }
}

fn day_bg_label(day_background: DayBackground) -> &'static str {
    match day_background {
        DayBackground::Suminagashi => "SUMINAGASHI",
        DayBackground::Shanshui => "SHANSHUI",
    }
}

fn overlay_label(overlay: OverlayMode) -> &'static str {
    match overlay {
        OverlayMode::None => "NONE",
        OverlayMode::Clock => "CLOCK",
    }
}

fn diag_label(kind: DiagKind) -> &'static str {
    match kind {
        DiagKind::None => "NONE",
        DiagKind::Debug => "DEBUG",
        DiagKind::Test => "TEST",
    }
}

fn write_diag_targets_label(
    out: &mut heapless::String<48>,
    targets: DiagTargets,
) -> Result<(), core::fmt::Error> {
    let bits = targets.as_persisted();
    if bits == 0 {
        return out.push_str("NONE").map_err(|_| core::fmt::Error);
    }

    let mut wrote_any = false;
    for (label, bit) in [
        ("SD", 1 << 0),
        ("WIFI", 1 << 1),
        ("DISPLAY", 1 << 2),
        ("TOUCH", 1 << 3),
        ("IMU", 1 << 4),
    ] {
        if (bits & bit) == 0 {
            continue;
        }
        if wrote_any {
            out.push('|').map_err(|_| core::fmt::Error)?;
        }
        out.push_str(label).map_err(|_| core::fmt::Error)?;
        wrote_any = true;
    }
    Ok(())
}

pub(super) async fn write_state_status_line(uart: &mut SerialUart) {
    let snapshot = read_app_state_snapshot();
    let mut targets = heapless::String::<48>::new();
    let _ = write_diag_targets_label(&mut targets, snapshot.diag_targets);
    let mut line = heapless::String::<256>::new();
    let _ = write!(
        &mut line,
        "STATE phase={} base={} day_bg={} overlay={} upload={} assets={} diag_kind={} targets={}\r\n",
        phase_label(snapshot.phase),
        base_label(snapshot.base),
        day_bg_label(snapshot.day_background),
        overlay_label(snapshot.overlay),
        if snapshot.services.upload_enabled {
            "on"
        } else {
            "off"
        },
        if snapshot.services.asset_reads_enabled {
            "on"
        } else {
            "off"
        },
        diag_label(snapshot.diag_kind),
        targets.as_str(),
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

pub(super) async fn write_diag_status_line(uart: &mut SerialUart) {
    let status = read_diag_runtime_status();
    let mut targets = heapless::String::<48>::new();
    let _ = write_diag_targets_label(&mut targets, DiagTargets::from_persisted(status.targets));
    let mut line = heapless::String::<160>::new();
    let _ = write!(
        &mut line,
        "DIAG state={} targets={} step={} code={}\r\n",
        status.state_label(),
        targets.as_str(),
        status.step_label(),
        status.code
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
