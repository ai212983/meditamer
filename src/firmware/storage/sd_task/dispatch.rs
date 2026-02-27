use embassy_time::{Instant, Timer};
use sdcard::runtime as sd_ops;

use super::super::super::types::{
    SdCommand, SdCommandKind, SdPowerRequest, SdProbeDriver, SdRequest, SdResult, SdResultCode,
};
use super::{duration_ms_since, request_sd_power, SD_RETRY_DELAY_MS, SD_RETRY_MAX_ATTEMPTS};

pub(super) async fn process_request(
    request: SdRequest,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    power: &mut impl FnMut(sd_ops::SdPowerAction) -> Result<(), ()>,
) -> SdResult {
    let kind = sd_command_kind(request.command);

    if !*powered {
        if !request_sd_power(SdPowerRequest::On).await {
            return SdResult {
                id: request.id,
                kind,
                ok: false,
                code: SdResultCode::PowerOnFailed,
                attempts: 0,
                duration_ms: 0,
            };
        }
        *powered = true;
    }

    if !matches!(request.command, SdCommand::Probe) && !sd_probe.is_initialized() {
        if let Err(err) = sd_probe.init().await {
            esp_println::println!("sdtask: init_error id={} err={:?}", request.id, err);
            return SdResult {
                id: request.id,
                kind,
                ok: false,
                code: SdResultCode::InitFailed,
                attempts: 0,
                duration_ms: 0,
            };
        }
    }

    let start = Instant::now();
    let mut attempts = 0u8;
    let mut code = SdResultCode::OperationFailed;

    while attempts < SD_RETRY_MAX_ATTEMPTS {
        attempts = attempts.saturating_add(1);
        code = run_sd_command("request", request.command, sd_probe, power).await;
        if code == SdResultCode::Ok {
            break;
        }
        if !sd_result_should_retry(code) {
            break;
        }

        if attempts < SD_RETRY_MAX_ATTEMPTS {
            Timer::after_millis(SD_RETRY_DELAY_MS).await;
            if !request_sd_power(SdPowerRequest::Off).await {
                let duration_ms = duration_ms_since(start);
                *powered = false;
                sd_probe.invalidate();
                return SdResult {
                    id: request.id,
                    kind,
                    ok: false,
                    code: SdResultCode::PowerOffFailed,
                    attempts,
                    duration_ms,
                };
            }
            *powered = false;
            sd_probe.invalidate();
            if !request_sd_power(SdPowerRequest::On).await {
                let duration_ms = duration_ms_since(start);
                return SdResult {
                    id: request.id,
                    kind,
                    ok: false,
                    code: SdResultCode::PowerOnFailed,
                    attempts,
                    duration_ms,
                };
            }
            *powered = true;
        }
    }

    let duration_ms = duration_ms_since(start);
    SdResult {
        id: request.id,
        kind,
        ok: code == SdResultCode::Ok,
        code,
        attempts,
        duration_ms,
    }
}

async fn run_sd_command(
    reason: &str,
    command: SdCommand,
    sd_probe: &mut SdProbeDriver,
    power: &mut impl FnMut(sd_ops::SdPowerAction) -> Result<(), ()>,
) -> SdResultCode {
    let power_mode = sd_ops::SdPowerMode::AlreadyOn;

    match command {
        SdCommand::Probe => sd_ops::run_sd_probe(reason, sd_probe, power, power_mode).await,
        SdCommand::RwVerify { lba } => {
            sd_ops::run_sd_rw_verify(reason, lba, sd_probe, power, power_mode).await
        }
        SdCommand::FatList { path, path_len } => {
            sd_ops::run_sd_fat_ls(reason, &path, path_len, sd_probe, power, power_mode).await
        }
        SdCommand::FatRead { path, path_len } => {
            sd_ops::run_sd_fat_read(reason, &path, path_len, sd_probe, power, power_mode).await
        }
        SdCommand::FatWrite {
            path,
            path_len,
            data,
            data_len,
        } => {
            sd_ops::run_sd_fat_write(
                reason, &path, path_len, &data, data_len, sd_probe, power, power_mode,
            )
            .await
        }
        SdCommand::FatStat { path, path_len } => {
            sd_ops::run_sd_fat_stat(reason, &path, path_len, sd_probe, power, power_mode).await
        }
        SdCommand::FatMkdir { path, path_len } => {
            sd_ops::run_sd_fat_mkdir(reason, &path, path_len, sd_probe, power, power_mode).await
        }
        SdCommand::FatRemove { path, path_len } => {
            sd_ops::run_sd_fat_remove(reason, &path, path_len, sd_probe, power, power_mode).await
        }
        SdCommand::FatRename {
            src_path,
            src_path_len,
            dst_path,
            dst_path_len,
        } => {
            sd_ops::run_sd_fat_rename(
                reason,
                &src_path,
                src_path_len,
                &dst_path,
                dst_path_len,
                sd_probe,
                power,
                power_mode,
            )
            .await
        }
        SdCommand::FatAppend {
            path,
            path_len,
            data,
            data_len,
        } => {
            sd_ops::run_sd_fat_append(
                reason, &path, path_len, &data, data_len, sd_probe, power, power_mode,
            )
            .await
        }
        SdCommand::FatTruncate {
            path,
            path_len,
            size,
        } => {
            sd_ops::run_sd_fat_truncate(reason, &path, path_len, size, sd_probe, power, power_mode)
                .await
        }
    }
}

pub(super) fn sd_result_should_retry(code: SdResultCode) -> bool {
    matches!(
        code,
        SdResultCode::PowerOnFailed | SdResultCode::InitFailed | SdResultCode::OperationFailed
    )
}

pub(super) fn sd_command_kind(command: SdCommand) -> SdCommandKind {
    match command {
        SdCommand::Probe => SdCommandKind::Probe,
        SdCommand::RwVerify { .. } => SdCommandKind::RwVerify,
        SdCommand::FatList { .. } => SdCommandKind::FatList,
        SdCommand::FatRead { .. } => SdCommandKind::FatRead,
        SdCommand::FatWrite { .. } => SdCommandKind::FatWrite,
        SdCommand::FatStat { .. } => SdCommandKind::FatStat,
        SdCommand::FatMkdir { .. } => SdCommandKind::FatMkdir,
        SdCommand::FatRemove { .. } => SdCommandKind::FatRemove,
        SdCommand::FatRename { .. } => SdCommandKind::FatRename,
        SdCommand::FatAppend { .. } => SdCommandKind::FatAppend,
        SdCommand::FatTruncate { .. } => SdCommandKind::FatTruncate,
    }
}
