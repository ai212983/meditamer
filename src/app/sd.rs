use embassy_futures::select::{select, Either};
use embassy_time::{with_timeout, Duration, Instant, Timer};
use sdcard::{fat, runtime as sd_ops};

use super::{
    config::{
        SD_POWER_REQUESTS, SD_POWER_RESPONSES, SD_REQUESTS, SD_RESULTS, SD_UPLOAD_REQUESTS,
        SD_UPLOAD_RESULTS,
    },
    types::{
        SdCommand, SdCommandKind, SdPowerRequest, SdProbeDriver, SdRequest, SdResult, SdResultCode,
        SdUploadCommand, SdUploadRequest, SdUploadResult, SdUploadResultCode,
    },
};

const SD_IDLE_POWER_OFF_MS: u64 = 1_500;
const SD_RETRY_MAX_ATTEMPTS: u8 = 3;
const SD_RETRY_DELAY_MS: u64 = 120;
const SD_BACKOFF_BASE_MS: u64 = 300;
const SD_BACKOFF_MAX_MS: u64 = 2_400;
const SD_POWER_RESPONSE_TIMEOUT_MS: u64 = 1_000;
const SD_UPLOAD_TMP_SUFFIX: &[u8] = b".part";
const SD_UPLOAD_PATH_BUF_MAX: usize = 72;

struct SdUploadSession {
    final_path: [u8; SD_UPLOAD_PATH_BUF_MAX],
    final_path_len: u8,
    temp_path: [u8; SD_UPLOAD_PATH_BUF_MAX],
    temp_path_len: u8,
    expected_size: u32,
    bytes_written: u32,
}

#[embassy_executor::task]
pub(crate) async fn sd_task(mut sd_probe: SdProbeDriver) {
    let mut powered = false;
    let mut upload_mounted = false;
    let mut upload_session: Option<SdUploadSession> = None;
    let mut no_power = |_action: sd_ops::SdPowerAction| -> Result<(), ()> { Ok(()) };
    let mut consecutive_failures = 0u8;
    let mut backoff_until: Option<Instant> = None;

    // Keep boot probe behavior, but now report completion through result channel.
    let boot_req = SdRequest {
        id: 0,
        command: SdCommand::Probe,
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
            match select(
                SD_UPLOAD_REQUESTS.receive(),
                with_timeout(
                    Duration::from_millis(SD_IDLE_POWER_OFF_MS),
                    SD_REQUESTS.receive(),
                ),
            )
            .await
            {
                Either::First(upload_request) => {
                    let result = process_upload_request(
                        upload_request,
                        &mut upload_session,
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await;
                    publish_upload_result(result);
                    continue;
                }
                Either::Second(result) => result.ok(),
            }
        } else {
            match select(SD_UPLOAD_REQUESTS.receive(), SD_REQUESTS.receive()).await {
                Either::First(upload_request) => {
                    let result = process_upload_request(
                        upload_request,
                        &mut upload_session,
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await;
                    publish_upload_result(result);
                    continue;
                }
                Either::Second(request) => Some(request),
            }
        };

        let Some(request) = request else {
            if powered && !request_sd_power(SdPowerRequest::Off).await {
                esp_println::println!("sdtask: idle_power_off_failed");
            }
            powered = false;
            upload_mounted = false;
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
            upload_mounted = false;
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

async fn process_upload_request(
    request: SdUploadRequest,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdUploadResult {
    match request.command {
        SdUploadCommand::Begin {
            path,
            path_len,
            expected_size,
        } => {
            if session.is_some() {
                return upload_result(false, SdUploadResultCode::Busy, 0);
            }

            let final_path = match parse_upload_path(&path, path_len) {
                Ok(path) => path,
                Err(code) => return upload_result(false, code, 0),
            };
            let final_path_bytes = final_path.as_bytes();

            if final_path_bytes.len() + SD_UPLOAD_TMP_SUFFIX.len() > SD_UPLOAD_PATH_BUF_MAX {
                return upload_result(false, SdUploadResultCode::InvalidPath, 0);
            }

            if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
                return upload_result(false, code, 0);
            }

            let mut temp_path = [0u8; SD_UPLOAD_PATH_BUF_MAX];
            temp_path[..final_path_bytes.len()].copy_from_slice(final_path_bytes);
            temp_path[final_path_bytes.len()..final_path_bytes.len() + SD_UPLOAD_TMP_SUFFIX.len()]
                .copy_from_slice(SD_UPLOAD_TMP_SUFFIX);
            let temp_len = final_path_bytes.len() + SD_UPLOAD_TMP_SUFFIX.len();
            let temp_path_str = match core::str::from_utf8(&temp_path[..temp_len]) {
                Ok(path) => path,
                Err(_) => return upload_result(false, SdUploadResultCode::InvalidPath, 0),
            };

            match fat::remove(sd_probe, temp_path_str).await {
                Ok(()) => {}
                Err(fat::SdFatError::NotFound) => {}
                Err(err) => {
                    return upload_result(false, map_fat_error_to_upload_code(&err), 0);
                }
            }

            if let Err(err) = fat::write_file(sd_probe, temp_path_str, &[]).await {
                return upload_result(false, map_fat_error_to_upload_code(&err), 0);
            }

            let mut final_path_buf = [0u8; SD_UPLOAD_PATH_BUF_MAX];
            final_path_buf[..final_path_bytes.len()].copy_from_slice(final_path_bytes);
            *session = Some(SdUploadSession {
                final_path: final_path_buf,
                final_path_len: final_path_bytes.len() as u8,
                temp_path,
                temp_path_len: temp_len as u8,
                expected_size,
                bytes_written: 0,
            });
            upload_result(true, SdUploadResultCode::Ok, 0)
        }
        SdUploadCommand::Chunk { data, data_len } => {
            let Some(active) = session.as_mut() else {
                return upload_result(false, SdUploadResultCode::SessionNotActive, 0);
            };
            let data_len = (data_len as usize).min(data.len());
            if data_len == 0 {
                return upload_result(true, SdUploadResultCode::Ok, active.bytes_written);
            }

            if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
                return upload_result(false, code, active.bytes_written);
            }

            let temp_path_str =
                match core::str::from_utf8(&active.temp_path[..active.temp_path_len as usize]) {
                    Ok(path) => path,
                    Err(_) => return upload_result(false, SdUploadResultCode::InvalidPath, 0),
                };
            if let Err(err) = fat::append_file(sd_probe, temp_path_str, &data[..data_len]).await {
                return upload_result(
                    false,
                    map_fat_error_to_upload_code(&err),
                    active.bytes_written,
                );
            }
            active.bytes_written = active.bytes_written.saturating_add(data_len as u32);
            upload_result(true, SdUploadResultCode::Ok, active.bytes_written)
        }
        SdUploadCommand::Commit => {
            let Some(active) = session.as_mut() else {
                return upload_result(false, SdUploadResultCode::SessionNotActive, 0);
            };
            if active.bytes_written != active.expected_size {
                return upload_result(
                    false,
                    SdUploadResultCode::SizeMismatch,
                    active.bytes_written,
                );
            }

            if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
                return upload_result(false, code, active.bytes_written);
            }

            let temp_path_str =
                match core::str::from_utf8(&active.temp_path[..active.temp_path_len as usize]) {
                    Ok(path) => path,
                    Err(_) => return upload_result(false, SdUploadResultCode::InvalidPath, 0),
                };
            let final_path_str =
                match core::str::from_utf8(&active.final_path[..active.final_path_len as usize]) {
                    Ok(path) => path,
                    Err(_) => return upload_result(false, SdUploadResultCode::InvalidPath, 0),
                };

            match fat::remove(sd_probe, final_path_str).await {
                Ok(()) => {}
                Err(fat::SdFatError::NotFound) => {}
                Err(err) => {
                    return upload_result(
                        false,
                        map_fat_error_to_upload_code(&err),
                        active.bytes_written,
                    );
                }
            }

            if let Err(err) = fat::rename(sd_probe, temp_path_str, final_path_str).await {
                return upload_result(
                    false,
                    map_fat_error_to_upload_code(&err),
                    active.bytes_written,
                );
            }
            let bytes_written = active.bytes_written;
            *session = None;
            upload_result(true, SdUploadResultCode::Ok, bytes_written)
        }
        SdUploadCommand::Abort => {
            let Some(active) = session.take() else {
                return upload_result(true, SdUploadResultCode::Ok, 0);
            };

            if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
                return upload_result(false, code, active.bytes_written);
            }

            let temp_path_str =
                match core::str::from_utf8(&active.temp_path[..active.temp_path_len as usize]) {
                    Ok(path) => path,
                    Err(_) => return upload_result(false, SdUploadResultCode::InvalidPath, 0),
                };
            match fat::remove(sd_probe, temp_path_str).await {
                Ok(()) | Err(fat::SdFatError::NotFound) => {
                    upload_result(true, SdUploadResultCode::Ok, active.bytes_written)
                }
                Err(err) => upload_result(
                    false,
                    map_fat_error_to_upload_code(&err),
                    active.bytes_written,
                ),
            }
        }
        SdUploadCommand::Mkdir { path, path_len } => {
            if session.is_some() {
                return upload_result(false, SdUploadResultCode::Busy, 0);
            }

            if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
                return upload_result(false, code, 0);
            }

            let path_str = match parse_upload_path(&path, path_len) {
                Ok(path) => path,
                Err(code) => return upload_result(false, code, 0),
            };

            match fat::mkdir(sd_probe, path_str).await {
                Ok(()) | Err(fat::SdFatError::AlreadyExists) => {
                    upload_result(true, SdUploadResultCode::Ok, 0)
                }
                Err(err) => upload_result(false, map_fat_error_to_upload_code(&err), 0),
            }
        }
        SdUploadCommand::Remove { path, path_len } => {
            if session.is_some() {
                return upload_result(false, SdUploadResultCode::Busy, 0);
            }

            if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
                return upload_result(false, code, 0);
            }

            let path_str = match parse_upload_path(&path, path_len) {
                Ok(path) => path,
                Err(code) => return upload_result(false, code, 0),
            };

            match fat::remove(sd_probe, path_str).await {
                Ok(()) | Err(fat::SdFatError::NotFound) => {
                    upload_result(true, SdUploadResultCode::Ok, 0)
                }
                Err(err) => upload_result(false, map_fat_error_to_upload_code(&err), 0),
            }
        }
    }
}

async fn ensure_upload_ready(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> Result<(), SdUploadResultCode> {
    if !*powered {
        if !request_sd_power(SdPowerRequest::On).await {
            return Err(SdUploadResultCode::PowerOnFailed);
        }
        *powered = true;
        *upload_mounted = false;
    }

    if !*upload_mounted {
        if sd_probe.init().await.is_err() {
            return Err(SdUploadResultCode::InitFailed);
        }
        *upload_mounted = true;
    }

    Ok(())
}

fn map_fat_error_to_upload_code(error: &fat::SdFatError) -> SdUploadResultCode {
    match error {
        fat::SdFatError::InvalidPath => SdUploadResultCode::InvalidPath,
        fat::SdFatError::NotFound => SdUploadResultCode::NotFound,
        fat::SdFatError::NotEmpty => SdUploadResultCode::NotEmpty,
        _ => SdUploadResultCode::OperationFailed,
    }
}

fn parse_upload_path(path: &[u8], path_len: u8) -> Result<&str, SdUploadResultCode> {
    let path_len = path_len as usize;
    if path_len == 0 || path_len > path.len() {
        return Err(SdUploadResultCode::InvalidPath);
    }
    let path_str =
        core::str::from_utf8(&path[..path_len]).map_err(|_| SdUploadResultCode::InvalidPath)?;
    if !path_str.starts_with('/') {
        return Err(SdUploadResultCode::InvalidPath);
    }
    Ok(path_str)
}

fn upload_result(ok: bool, code: SdUploadResultCode, bytes_written: u32) -> SdUploadResult {
    SdUploadResult {
        ok,
        code,
        bytes_written,
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
    while SD_POWER_RESPONSES.try_receive().is_ok() {}

    if SD_POWER_REQUESTS.try_send(action).is_err() {
        esp_println::println!(
            "sdtask: power_req_queue_full action={}",
            sd_power_action_label(action)
        );
        return false;
    }

    match with_timeout(
        Duration::from_millis(SD_POWER_RESPONSE_TIMEOUT_MS),
        SD_POWER_RESPONSES.receive(),
    )
    .await
    {
        Ok(ok) => ok,
        Err(_) => {
            esp_println::println!(
                "sdtask: power_resp_timeout action={} timeout_ms={}",
                sd_power_action_label(action),
                SD_POWER_RESPONSE_TIMEOUT_MS
            );
            false
        }
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

fn sd_result_should_retry(code: SdResultCode) -> bool {
    matches!(
        code,
        SdResultCode::PowerOnFailed | SdResultCode::InitFailed | SdResultCode::OperationFailed
    )
}

fn sd_command_kind(command: SdCommand) -> SdCommandKind {
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

fn publish_upload_result(result: SdUploadResult) {
    if SD_UPLOAD_RESULTS.try_send(result).is_err() {
        esp_println::println!(
            "sdtask: upload_result_drop ok={} code={} bytes_written={}",
            result.ok as u8,
            sd_upload_result_code_label(result.code),
            result.bytes_written
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
        SdUploadResultCode::OperationFailed => "operation_failed",
    }
}

fn sd_power_action_label(action: SdPowerRequest) -> &'static str {
    match action {
        SdPowerRequest::On => "on",
        SdPowerRequest::Off => "off",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::types::SD_PATH_MAX;

    #[test]
    fn backoff_grows_and_clamps() {
        assert_eq!(failure_backoff_ms(0), SD_BACKOFF_BASE_MS);
        assert_eq!(failure_backoff_ms(1), SD_BACKOFF_BASE_MS);
        assert_eq!(failure_backoff_ms(2), SD_BACKOFF_BASE_MS * 2);
        assert_eq!(failure_backoff_ms(3), SD_BACKOFF_BASE_MS * 4);
        assert_eq!(failure_backoff_ms(8), SD_BACKOFF_MAX_MS);
        assert_eq!(failure_backoff_ms(32), SD_BACKOFF_MAX_MS);
    }

    #[test]
    fn command_kind_mapping_is_stable() {
        assert_eq!(sd_command_kind(SdCommand::Probe), SdCommandKind::Probe);
        assert_eq!(
            sd_command_kind(SdCommand::RwVerify { lba: 1 }),
            SdCommandKind::RwVerify
        );
        assert_eq!(
            sd_command_kind(SdCommand::FatStat {
                path: [0; SD_PATH_MAX],
                path_len: 0
            }),
            SdCommandKind::FatStat
        );
        assert_eq!(
            sd_command_kind(SdCommand::FatTruncate {
                path: [0; SD_PATH_MAX],
                path_len: 0,
                size: 0
            }),
            SdCommandKind::FatTruncate
        );
    }

    #[test]
    fn retry_policy_matches_result_codes() {
        assert!(sd_result_should_retry(SdResultCode::PowerOnFailed));
        assert!(sd_result_should_retry(SdResultCode::InitFailed));
        assert!(sd_result_should_retry(SdResultCode::OperationFailed));
        assert!(!sd_result_should_retry(SdResultCode::InvalidPath));
        assert!(!sd_result_should_retry(SdResultCode::NotFound));
        assert!(!sd_result_should_retry(SdResultCode::VerifyMismatch));
        assert!(!sd_result_should_retry(SdResultCode::PowerOffFailed));
        assert!(!sd_result_should_retry(SdResultCode::RefusedLba0));
    }
}
