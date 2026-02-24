use embassy_time::{with_timeout, Duration, Instant, Timer};
use sdcard::runtime as sd_ops;

use super::{
    config::{SD_POWER_REQUESTS, SD_POWER_RESPONSES, SD_REQUESTS, SD_RESULTS},
    types::{
        SdCommand, SdCommandKind, SdPowerRequest, SdProbeDriver, SdRequest, SdResult, SdResultCode,
    },
};

const SD_IDLE_POWER_OFF_MS: u64 = 1_500;
const SD_RETRY_MAX_ATTEMPTS: u8 = 3;
const SD_RETRY_DELAY_MS: u64 = 120;
const SD_BACKOFF_BASE_MS: u64 = 300;
const SD_BACKOFF_MAX_MS: u64 = 2_400;

#[embassy_executor::task]
pub(crate) async fn sd_task(mut sd_probe: SdProbeDriver) {
    let mut powered = false;
    let mut no_power = |_action: sd_ops::SdPowerAction| -> Result<(), ()> { Ok(()) };
    let mut consecutive_failures = 0u8;
    let mut backoff_until: Option<Instant> = None;

    // Keep boot probe behavior, but now report completion through result channel.
    let boot_req = SdRequest {
        id: 0,
        command: SdCommand::SdProbe,
    };
    let boot_result = process_request(boot_req, &mut sd_probe, &mut powered, &mut no_power).await;
    publish_result(boot_result);
    if !boot_result.ok {
        consecutive_failures = 1;
        backoff_until = Some(Instant::now() + Duration::from_millis(failure_backoff_ms(1)));
    }

    loop {
        if let Some(until) = backoff_until {
            let now = Instant::now();
            if now < until {
                Timer::after(until.saturating_duration_since(now)).await;
            }
            backoff_until = None;
        }

        let request = if powered {
            match with_timeout(
                Duration::from_millis(SD_IDLE_POWER_OFF_MS),
                SD_REQUESTS.receive(),
            )
            .await
            {
                Ok(request) => Some(request),
                Err(_) => None,
            }
        } else {
            Some(SD_REQUESTS.receive().await)
        };

        let Some(request) = request else {
            if powered && !request_sd_power(SdPowerRequest::Off).await {
                esp_println::println!("sdtask: idle_power_off_failed");
            }
            powered = false;
            continue;
        };

        let result = process_request(request, &mut sd_probe, &mut powered, &mut no_power).await;
        publish_result(result);

        if result.ok {
            consecutive_failures = 0;
            backoff_until = None;
        } else {
            consecutive_failures = consecutive_failures.saturating_add(1).min(8);
            let backoff_ms = failure_backoff_ms(consecutive_failures);
            backoff_until = Some(Instant::now() + Duration::from_millis(backoff_ms));
            if powered && !request_sd_power(SdPowerRequest::Off).await {
                esp_println::println!("sdtask: fail_power_off_failed");
            }
            powered = false;
        }
    }
}

async fn process_request(
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

    let start = Instant::now();
    let mut attempts = 0u8;
    let mut ok = false;

    while attempts < SD_RETRY_MAX_ATTEMPTS {
        attempts = attempts.saturating_add(1);
        ok = run_sd_command("request", request.command, sd_probe, power).await;
        if ok {
            break;
        }

        if attempts < SD_RETRY_MAX_ATTEMPTS {
            Timer::after_millis(SD_RETRY_DELAY_MS).await;
            if !request_sd_power(SdPowerRequest::Off).await {
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
            *powered = false;
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
        ok,
        code: if ok {
            SdResultCode::Ok
        } else {
            SdResultCode::OperationFailed
        },
        attempts,
        duration_ms,
    }
}

fn duration_ms_since(start: Instant) -> u32 {
    Instant::now()
        .saturating_duration_since(start)
        .as_millis()
        .min(u32::MAX as u64) as u32
}

fn failure_backoff_ms(consecutive_failures: u8) -> u64 {
    let exponent = consecutive_failures.saturating_sub(1).min(6);
    let factor = 1u64 << exponent;
    SD_BACKOFF_BASE_MS
        .saturating_mul(factor)
        .min(SD_BACKOFF_MAX_MS)
}

async fn request_sd_power(action: SdPowerRequest) -> bool {
    SD_POWER_REQUESTS.send(action).await;
    SD_POWER_RESPONSES.receive().await
}

async fn run_sd_command(
    reason: &str,
    command: SdCommand,
    sd_probe: &mut SdProbeDriver,
    power: &mut impl FnMut(sd_ops::SdPowerAction) -> Result<(), ()>,
) -> bool {
    match command {
        SdCommand::SdProbe => sd_ops::run_sd_probe(reason, sd_probe, power).await,
        SdCommand::SdRwVerify { lba } => {
            sd_ops::run_sd_rw_verify(reason, lba, sd_probe, power).await
        }
        SdCommand::SdFatList { path, path_len } => {
            sd_ops::run_sd_fat_ls(reason, &path, path_len, sd_probe, power).await
        }
        SdCommand::SdFatRead { path, path_len } => {
            sd_ops::run_sd_fat_read(reason, &path, path_len, sd_probe, power).await
        }
        SdCommand::SdFatWrite {
            path,
            path_len,
            data,
            data_len,
        } => {
            sd_ops::run_sd_fat_write(reason, &path, path_len, &data, data_len, sd_probe, power)
                .await
        }
        SdCommand::SdFatStat { path, path_len } => {
            sd_ops::run_sd_fat_stat(reason, &path, path_len, sd_probe, power).await
        }
        SdCommand::SdFatMkdir { path, path_len } => {
            sd_ops::run_sd_fat_mkdir(reason, &path, path_len, sd_probe, power).await
        }
        SdCommand::SdFatRemove { path, path_len } => {
            sd_ops::run_sd_fat_remove(reason, &path, path_len, sd_probe, power).await
        }
        SdCommand::SdFatRename {
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
            )
            .await
        }
        SdCommand::SdFatAppend {
            path,
            path_len,
            data,
            data_len,
        } => {
            sd_ops::run_sd_fat_append(reason, &path, path_len, &data, data_len, sd_probe, power)
                .await
        }
        SdCommand::SdFatTruncate {
            path,
            path_len,
            size,
        } => sd_ops::run_sd_fat_truncate(reason, &path, path_len, size, sd_probe, power).await,
    }
}

fn sd_command_kind(command: SdCommand) -> SdCommandKind {
    match command {
        SdCommand::SdProbe => SdCommandKind::Probe,
        SdCommand::SdRwVerify { .. } => SdCommandKind::RwVerify,
        SdCommand::SdFatList { .. } => SdCommandKind::FatList,
        SdCommand::SdFatRead { .. } => SdCommandKind::FatRead,
        SdCommand::SdFatWrite { .. } => SdCommandKind::FatWrite,
        SdCommand::SdFatStat { .. } => SdCommandKind::FatStat,
        SdCommand::SdFatMkdir { .. } => SdCommandKind::FatMkdir,
        SdCommand::SdFatRemove { .. } => SdCommandKind::FatRemove,
        SdCommand::SdFatRename { .. } => SdCommandKind::FatRename,
        SdCommand::SdFatAppend { .. } => SdCommandKind::FatAppend,
        SdCommand::SdFatTruncate { .. } => SdCommandKind::FatTruncate,
    }
}

fn publish_result(result: SdResult) {
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

fn sd_result_code_label(code: SdResultCode) -> &'static str {
    match code {
        SdResultCode::Ok => "ok",
        SdResultCode::PowerOnFailed => "power_on_failed",
        SdResultCode::OperationFailed => "operation_failed",
    }
}
