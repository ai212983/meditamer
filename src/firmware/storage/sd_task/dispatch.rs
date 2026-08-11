use super::engine_driver::run_fat_engine_command;
use super::manual_io::{run_probe, run_rw_verify};
use embassy_time::{with_timeout, Duration, Instant, Timer};
use sdcard::fat::FatEngine;
use sdcard::runtime as sd_ops;

use super::super::super::types::{
    SdCommand, SdCommandKind, SdPowerRequest, SdProbeDriver, SdRequest, SdResult, SdResultCode,
};
use super::{
    duration_ms_since, request_sd_power, SD_POWER_CYCLE_OFF_MS, SD_RETRY_DELAY_MS,
    SD_RETRY_MAX_ATTEMPTS,
};

pub(super) async fn process_request(
    request: SdRequest,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    power: &mut impl FnMut(sd_ops::SdPowerAction) -> Result<(), ()>,
    fat_engine: &mut FatEngine,
) -> SdResult {
    let kind = sd_command_kind(request.command);

    if let Some(result) = ensure_powered_for_request(request.id, kind, powered).await {
        return result;
    }

    if let Some(result) = ensure_initialized_for_request(&request, kind, sd_probe, fat_engine).await
    {
        return result;
    }

    run_request_with_retries(request, kind, sd_probe, powered, power, fat_engine).await
}

async fn ensure_powered_for_request(
    request_id: u32,
    kind: SdCommandKind,
    powered: &mut bool,
) -> Option<SdResult> {
    if *powered {
        return None;
    }
    if request_sd_power(SdPowerRequest::On).await {
        *powered = true;
        return None;
    }
    Some(SdResult {
        id: request_id,
        kind,
        ok: false,
        code: SdResultCode::PowerOnFailed,
        attempts: 0,
        duration_ms: 0,
        recover_bus: true,
    })
}

async fn ensure_initialized_for_request(
    request: &SdRequest,
    kind: SdCommandKind,
    sd_probe: &mut SdProbeDriver,
    fat_engine: &mut FatEngine,
) -> Option<SdResult> {
    if matches!(request.command, SdCommand::Probe) || sd_probe.is_initialized() {
        return None;
    }

    let init = match with_timeout(Duration::from_secs(2), sd_probe.init()).await {
        Ok(result) => result,
        Err(err) => {
            sd_probe.recover_after_timeout();
            fat_engine.invalidate();
            esp_println::println!("sdtask: init_timeout id={} err={:?}", request.id, err);
            return Some(SdResult {
                id: request.id,
                kind,
                ok: false,
                code: SdResultCode::InitFailed,
                attempts: 0,
                duration_ms: 0,
                recover_bus: true,
            });
        }
    };
    if let Err(err) = init {
        sd_probe.recover_after_timeout();
        fat_engine.invalidate();
        esp_println::println!("sdtask: init_error id={} err={:?}", request.id, err);
        return Some(SdResult {
            id: request.id,
            kind,
            ok: false,
            code: SdResultCode::InitFailed,
            attempts: 0,
            duration_ms: 0,
            recover_bus: true,
        });
    }

    None
}

async fn run_request_with_retries(
    request: SdRequest,
    kind: SdCommandKind,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    power: &mut impl FnMut(sd_ops::SdPowerAction) -> Result<(), ()>,
    fat_engine: &mut FatEngine,
) -> SdResult {
    let start = Instant::now();
    let mut attempts = 0u8;
    let mut code = SdResultCode::OperationFailed;
    let mut recover_bus = false;

    while attempts < SD_RETRY_MAX_ATTEMPTS {
        attempts = attempts.saturating_add(1);
        let retryable;
        code = match reinitialize_after_retry(request.id, sd_probe, fat_engine).await {
            Ok(()) => {
                let outcome =
                    run_sd_command("request", request.command, sd_probe, power, fat_engine).await;
                retryable = outcome.1;
                outcome.0
            }
            Err(init_code) => {
                retryable = sd_result_should_retry(init_code);
                init_code
            }
        };
        recover_bus = retryable;
        if code == SdResultCode::Ok {
            break;
        }
        if !retryable {
            break;
        }

        if attempts < SD_RETRY_MAX_ATTEMPTS {
            if let Some(result) = retry_after_power_cycle(
                start, request.id, kind, attempts, sd_probe, powered, fat_engine,
            )
            .await
            {
                return result;
            }
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
        recover_bus: code != SdResultCode::Ok && recover_bus,
    }
}

async fn reinitialize_after_retry(
    request_id: u32,
    sd_probe: &mut SdProbeDriver,
    fat_engine: &mut FatEngine,
) -> Result<(), SdResultCode> {
    if sd_probe.is_initialized() {
        return Ok(());
    }

    match with_timeout(Duration::from_secs(2), sd_probe.init()).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(err)) => {
            sd_probe.recover_after_timeout();
            fat_engine.invalidate();
            esp_println::println!("sdtask: retry_init_error id={} err={:?}", request_id, err);
            Err(SdResultCode::InitFailed)
        }
        Err(err) => {
            sd_probe.recover_after_timeout();
            fat_engine.invalidate();
            esp_println::println!("sdtask: retry_init_timeout id={} err={:?}", request_id, err);
            Err(SdResultCode::InitFailed)
        }
    }
}

async fn retry_after_power_cycle(
    start: Instant,
    request_id: u32,
    kind: SdCommandKind,
    attempts: u8,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    fat_engine: &mut FatEngine,
) -> Option<SdResult> {
    Timer::after_millis(SD_RETRY_DELAY_MS).await;

    if !request_sd_power(SdPowerRequest::Off).await {
        *powered = false;
        sd_probe.invalidate();
        fat_engine.invalidate();
        return Some(SdResult {
            id: request_id,
            kind,
            ok: false,
            code: SdResultCode::PowerOffFailed,
            attempts,
            duration_ms: duration_ms_since(start),
            recover_bus: true,
        });
    }

    *powered = false;
    sd_probe.invalidate();
    fat_engine.invalidate();
    Timer::after_millis(SD_POWER_CYCLE_OFF_MS).await;

    if request_sd_power(SdPowerRequest::On).await {
        *powered = true;
        return None;
    }

    Some(SdResult {
        id: request_id,
        kind,
        ok: false,
        code: SdResultCode::PowerOnFailed,
        attempts,
        duration_ms: duration_ms_since(start),
        recover_bus: true,
    })
}

async fn run_sd_command(
    reason: &str,
    command: SdCommand,
    sd_probe: &mut SdProbeDriver,
    _power: &mut impl FnMut(sd_ops::SdPowerAction) -> Result<(), ()>,
    fat_engine: &mut FatEngine,
) -> (SdResultCode, bool) {
    let code = match command {
        SdCommand::Probe => run_probe(reason, sd_probe).await,
        SdCommand::RwVerify { lba } => run_rw_verify(reason, lba, sd_probe).await,
        command => return run_fat_engine_command(command, sd_probe, fat_engine).await,
    };
    (code, sd_result_should_retry(code))
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
