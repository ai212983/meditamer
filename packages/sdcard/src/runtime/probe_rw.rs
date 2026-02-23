use core::fmt::Write;

use crate::{power_off, power_on_for_io, fat, probe, SD_WRITE_MAX};

#[derive(Clone, Copy)]
pub enum SdPowerAction {
    On,
    Off,
}

pub async fn run_sd_probe<E, P>(
    reason: &str,
    sd_probe: &mut probe::SdCardProbe<'_>,
    power: &mut P,
) where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    if power_on(power).await.is_err() {
        esp_println::println!("sdprobe[{}]: power_on_error", reason);
        return;
    }

    let result = sd_probe.probe().await;

    match result {
        Ok(status) => {
            let version = match status.version {
                probe::SdCardVersion::V1 => "v1.x",
                probe::SdCardVersion::V2 => "v2+",
            };
            let capacity = if status.high_capacity {
                "sdhc_or_sdxc"
            } else {
                "sdsc"
            };
            let filesystem = match status.filesystem {
                probe::SdFilesystem::ExFat => "exfat",
                probe::SdFilesystem::Fat32 => "fat32",
                probe::SdFilesystem::Fat16 => "fat16",
                probe::SdFilesystem::Fat12 => "fat12",
                probe::SdFilesystem::Ntfs => "ntfs",
                probe::SdFilesystem::Unknown => "unknown",
            };
            let gib_x100 = status
                .capacity_bytes
                .saturating_mul(100)
                .saturating_div(1024 * 1024 * 1024);
            let gib_int = gib_x100 / 100;
            let gib_frac = gib_x100 % 100;
            esp_println::println!(
                "sdprobe[{}]: card_detected version={} capacity={} fs={} bytes={} size_gib={}.{:02}",
                reason,
                version,
                capacity,
                filesystem,
                status.capacity_bytes,
                gib_int,
                gib_frac
            );
        }
        Err(err) => match err {
            probe::SdProbeError::Spi(spi_err) => {
                esp_println::println!("sdprobe[{}]: not_detected spi_error={:?}", reason, spi_err);
            }
            probe::SdProbeError::Cmd0Failed(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd0_r1=0x{:02x}", reason, r1);
            }
            probe::SdProbeError::Cmd8Unexpected(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd8_r1=0x{:02x}", reason, r1);
            }
            probe::SdProbeError::Cmd8EchoMismatch(r7) => {
                esp_println::println!(
                    "sdprobe[{}]: not_detected cmd8_echo={:02x}{:02x}{:02x}{:02x}",
                    reason,
                    r7[0],
                    r7[1],
                    r7[2],
                    r7[3]
                );
            }
            probe::SdProbeError::Acmd41Timeout(r1) => {
                esp_println::println!(
                    "sdprobe[{}]: not_detected acmd41_last_r1=0x{:02x}",
                    reason,
                    r1
                );
            }
            probe::SdProbeError::Cmd58Unexpected(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd58_r1=0x{:02x}", reason, r1);
            }
            probe::SdProbeError::Cmd9Unexpected(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd9_r1=0x{:02x}", reason, r1);
            }
            probe::SdProbeError::Cmd16Unexpected(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd16_r1=0x{:02x}", reason, r1);
            }
            probe::SdProbeError::Cmd17Unexpected(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd17_r1=0x{:02x}", reason, r1);
            }
            probe::SdProbeError::Cmd24Unexpected(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd24_r1=0x{:02x}", reason, r1);
            }
            probe::SdProbeError::NoResponse(cmd) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd{}_no_response", reason, cmd);
            }
            probe::SdProbeError::DataTokenTimeout(cmd) => {
                esp_println::println!(
                    "sdprobe[{}]: not_detected cmd{}_data_token_timeout",
                    reason,
                    cmd
                );
            }
            probe::SdProbeError::DataTokenUnexpected(cmd, token) => {
                esp_println::println!(
                    "sdprobe[{}]: not_detected cmd{}_data_token=0x{:02x}",
                    reason,
                    cmd,
                    token
                );
            }
            probe::SdProbeError::WriteDataRejected(response) => {
                esp_println::println!(
                    "sdprobe[{}]: not_detected write_response=0x{:02x}",
                    reason,
                    response
                );
            }
            probe::SdProbeError::WriteBusyTimeout => {
                esp_println::println!("sdprobe[{}]: not_detected write_busy_timeout", reason);
            }
            probe::SdProbeError::NotInitialized => {
                esp_println::println!("sdprobe[{}]: not_detected not_initialized", reason);
            }
            probe::SdProbeError::CapacityDecodeFailed => {
                esp_println::println!("sdprobe[{}]: not_detected capacity_decode_failed", reason);
            }
        },
    }

    if power_off_io(power).is_err() {
        esp_println::println!("sdprobe[{}]: power_off_error", reason);
    }
}

pub async fn run_sd_rw_verify<E, P>(
    reason: &str,
    lba: u32,
    sd_probe: &mut probe::SdCardProbe<'_>,
    power: &mut P,
) where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    if lba == 0 {
        esp_println::println!("sdrw[{}]: refused_lba0", reason);
        return;
    }

    if power_on(power).await.is_err() {
        esp_println::println!("sdrw[{}]: power_on_error", reason);
        return;
    }

    if let Err(err) = sd_probe.init().await {
        esp_println::println!("sdrw[{}]: init_error={:?}", reason, err);
        let _ = power_off_io(power);
        return;
    }

    let mut before = [0u8; probe::SD_SECTOR_SIZE];
    if let Err(err) = sd_probe.read_sector(lba, &mut before).await {
        esp_println::println!(
            "sdrw[{}]: read_before_error lba={} err={:?}",
            reason,
            lba,
            err
        );
        let _ = power_off_io(power);
        return;
    }

    if let Err(err) = sd_probe.write_sector(lba, &before).await {
        esp_println::println!("sdrw[{}]: write_error lba={} err={:?}", reason, lba, err);
        let _ = power_off_io(power);
        return;
    }

    let mut after = [0u8; probe::SD_SECTOR_SIZE];
    if let Err(err) = sd_probe.read_sector(lba, &mut after).await {
        esp_println::println!(
            "sdrw[{}]: read_after_error lba={} err={:?}",
            reason,
            lba,
            err
        );
        let _ = power_off_io(power);
        return;
    }

    if let Some(idx) = before.iter().zip(after.iter()).position(|(a, b)| a != b) {
        esp_println::println!(
            "sdrw[{}]: verify_mismatch lba={} byte={} before=0x{:02x} after=0x{:02x}",
            reason,
            lba,
            idx,
            before[idx],
            after[idx]
        );
    } else {
        esp_println::println!(
            "sdrw[{}]: verify_ok lba={} bytes={}",
            reason,
            lba,
            probe::SD_SECTOR_SIZE
        );
    }

    if power_off_io(power).is_err() {
        esp_println::println!("sdrw[{}]: power_off_error", reason);
    }
}

