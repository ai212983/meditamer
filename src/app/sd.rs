use embassy_time::{with_timeout, Duration, Instant};
use sdcard::runtime as sd_ops;

use super::{
    config::{SD_POWER_REQUESTS, SD_POWER_RESPONSES, SD_REQUESTS, SD_RESULTS},
    types::{SdCommand, SdCommandKind, SdPowerRequest, SdProbeDriver, SdRequest, SdResult},
};

const SD_IDLE_POWER_OFF_MS: u64 = 1_500;

#[embassy_executor::task]
pub(crate) async fn sd_task(mut sd_probe: SdProbeDriver) {
    let mut powered = false;
    let mut no_power = |_action: sd_ops::SdPowerAction| -> Result<(), ()> { Ok(()) };

    // Keep boot probe behavior, but now report completion through result channel.
    let boot_req = SdRequest {
        id: 0,
        command: SdCommand::SdProbe,
    };
    let _ = process_request(boot_req, &mut sd_probe, &mut powered, &mut no_power).await;

    loop {
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
            if powered {
                if !request_sd_power(SdPowerRequest::Off).await {
                    esp_println::println!("sdtask: idle_power_off_failed");
                }
                powered = false;
            }
            continue;
        };

        let _ = process_request(request, &mut sd_probe, &mut powered, &mut no_power).await;
    }
}

async fn process_request(
    request: SdRequest,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    power: &mut impl FnMut(sd_ops::SdPowerAction) -> Result<(), ()>,
) -> bool {
    if !*powered {
        if !request_sd_power(SdPowerRequest::On).await {
            publish_result(request.id, sd_command_kind(request.command), false, 0);
            return false;
        }
        *powered = true;
    }

    let start = Instant::now();
    let ok = run_sd_command("request", request.command, sd_probe, power).await;
    let duration_ms = Instant::now()
        .saturating_duration_since(start)
        .as_millis()
        .min(u32::MAX as u64) as u32;
    publish_result(
        request.id,
        sd_command_kind(request.command),
        ok,
        duration_ms,
    );
    ok
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

fn publish_result(id: u32, kind: SdCommandKind, ok: bool, duration_ms: u32) {
    let result = SdResult {
        id,
        kind,
        ok,
        duration_ms,
    };
    if SD_RESULTS.try_send(result).is_err() {
        esp_println::println!(
            "sdtask: result_drop id={} kind={} ok={} dur_ms={}",
            id,
            sd_kind_label(kind),
            ok as u8,
            duration_ms
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
