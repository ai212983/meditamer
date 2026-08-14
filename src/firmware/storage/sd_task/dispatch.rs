use super::engine_driver::run_fat_engine_command;
use super::manual_io::{run_probe, run_rw_verify};
use embassy_time::{Duration, Instant, Timer};
use sdcard::fat::FatEngine;
use sdcard::runtime as sd_ops;

use super::super::super::types::{
    SdCommand, SdCommandKind, SdPowerRequest, SdProbeDriver, SdRequest, SdResult, SdResultCode,
};
use super::{duration_ms_since, request_sd_power, SD_RETRY_DELAY_MS, SD_RETRY_MAX_ATTEMPTS};

// This stays below the 10-second serial/upload caller timeout and is shared by
// every initialization attempt in one SD request. SdCardProbe checks it only
// between completed DMA transfers, so expiry never cancels an in-flight DMA.
const SD_REQUEST_INIT_DEADLINE_MS: u64 = 8_000;
const SD_POWER_CYCLE_OFF_MS: u64 = 250;

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
    let init_deadline = Instant::now() + Duration::from_millis(SD_REQUEST_INIT_DEADLINE_MS);

    if let Some(result) =
        ensure_initialized_for_request(&request, kind, sd_probe, fat_engine, init_deadline).await
    {
        return result;
    }

    run_request_with_retries(
        request,
        kind,
        sd_probe,
        powered,
        power,
        fat_engine,
        init_deadline,
    )
    .await
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
    init_deadline: Instant,
) -> Option<SdResult> {
    if matches!(request.command, SdCommand::Probe) || sd_probe.is_initialized() {
        return None;
    }

    // SdCardProbe owns each DMA transfer through completion, bounds protocol
    // loops, and checks this overall deadline between transfers. Do not wrap
    // initialization in a timeout that can drop the owned DMA future.
    let init = sd_probe.init_until(init_deadline).await;
    if let Err(err) = init {
        sd_probe.recover_after_timeout();
        fat_engine.invalidate();
        if !crate::firmware::update::transport_quiet() {
            esp_println::println!("sdtask: init_error id={} err={:?}", request.id, err);
        }
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
    init_deadline: Instant,
) -> SdResult {
    let start = Instant::now();
    let mut attempts = 0u8;
    let mut code = SdResultCode::OperationFailed;
    let mut recover_bus = false;

    while attempts < SD_RETRY_MAX_ATTEMPTS {
        attempts = attempts.saturating_add(1);
        let retryable;
        let retry_requires_power_cycle;
        code = match reinitialize_after_retry(request.id, sd_probe, fat_engine, init_deadline).await
        {
            Ok(()) => {
                let outcome = run_sd_command(
                    "request",
                    request.command,
                    sd_probe,
                    power,
                    fat_engine,
                    init_deadline,
                )
                .await;
                retryable = outcome.1;
                retry_requires_power_cycle =
                    operation_retry_requires_power_cycle(request.command, outcome.0, retryable);
                outcome.0
            }
            Err(init_code) => {
                retryable = sd_result_should_retry(init_code);
                retry_requires_power_cycle = false;
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
            sd_probe.recover_after_timeout();
            fat_engine.invalidate();
            if retry_requires_power_cycle {
                match retry_after_operation_power_cycle(
                    powered,
                    sd_probe,
                    fat_engine,
                    init_deadline,
                )
                .await
                {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(power_code) => {
                        return SdResult {
                            id: request.id,
                            kind,
                            ok: false,
                            code: power_code,
                            attempts,
                            duration_ms: duration_ms_since(start),
                            recover_bus: true,
                        };
                    }
                }
            } else if !wait_before_deadline(SD_RETRY_DELAY_MS, init_deadline).await {
                // Cooperative initialization expiry already leaves CS high
                // and no transfer in flight. Stop rather than power-cycling
                // the card after the request's absolute deadline.
                break;
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

pub(super) fn operation_retry_requires_power_cycle(
    command: SdCommand,
    code: SdResultCode,
    retryable: bool,
) -> bool {
    retryable && code == SdResultCode::OperationFailed && !matches!(command, SdCommand::Probe)
}

async fn retry_after_operation_power_cycle(
    powered: &mut bool,
    sd_probe: &mut SdProbeDriver,
    fat_engine: &mut FatEngine,
    deadline: Instant,
) -> Result<bool, SdResultCode> {
    if !wait_before_deadline(SD_RETRY_DELAY_MS, deadline).await {
        return Ok(false);
    }

    // The failed operation has completed and recovery raised CS, so no DMA
    // future exists when the physical card rail is switched off.
    if !request_sd_power(SdPowerRequest::Off).await {
        *powered = false;
        sd_probe.invalidate();
        fat_engine.invalidate();
        return Err(SdResultCode::PowerOffFailed);
    }

    *powered = false;
    sd_probe.invalidate();
    fat_engine.invalidate();
    if !wait_before_deadline(SD_POWER_CYCLE_OFF_MS, deadline).await {
        // Leave the rail off. The next request will perform a normal power-on
        // instead of scheduling recovery work beyond this request's deadline.
        return Ok(false);
    }

    if request_sd_power(SdPowerRequest::On).await {
        *powered = true;
        Ok(Instant::now() < deadline)
    } else {
        Err(SdResultCode::PowerOnFailed)
    }
}

async fn reinitialize_after_retry(
    request_id: u32,
    sd_probe: &mut SdProbeDriver,
    fat_engine: &mut FatEngine,
    init_deadline: Instant,
) -> Result<(), SdResultCode> {
    if sd_probe.is_initialized() {
        return Ok(());
    }

    match sd_probe.init_until(init_deadline).await {
        Ok(_) => Ok(()),
        Err(err) => {
            sd_probe.recover_after_timeout();
            fat_engine.invalidate();
            if !crate::firmware::update::transport_quiet() {
                esp_println::println!("sdtask: retry_init_error id={} err={:?}", request_id, err);
            }
            Err(SdResultCode::InitFailed)
        }
    }
}

async fn wait_before_deadline(delay_ms: u64, deadline: Instant) -> bool {
    let wake_at = Instant::now() + Duration::from_millis(delay_ms);
    if wake_at >= deadline {
        return false;
    }
    Timer::at(wake_at).await;
    Instant::now() < deadline
}

async fn run_sd_command(
    reason: &str,
    command: SdCommand,
    sd_probe: &mut SdProbeDriver,
    _power: &mut impl FnMut(sd_ops::SdPowerAction) -> Result<(), ()>,
    fat_engine: &mut FatEngine,
    init_deadline: Instant,
) -> (SdResultCode, bool) {
    let code = match command {
        SdCommand::Probe => run_probe(reason, sd_probe, init_deadline).await,
        SdCommand::RwVerify { lba } => run_rw_verify(reason, lba, sd_probe, init_deadline).await,
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
