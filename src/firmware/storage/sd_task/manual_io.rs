use core::fmt::Write;

use sdcard::probe::{
    SdCardVersion, SdFilesystem, SdProbeError, SdProbeStatus, SD_SECTOR_SIZE,
};

use super::serial_log::{self, SdSerialLine};
use crate::firmware::types::{SdProbeDriver, SdResultCode};

fn send_line(line: SdSerialLine) {
    let _ = serial_log::send(line);
}

macro_rules! queue_line {
    ($($arg:tt)*) => {{
        let mut line = SdSerialLine::new();
        let _ = write!(&mut line, $($arg)*);
        let _ = line.push_str("\r\n");
        send_line(line);
    }};
}

pub(super) async fn run_probe(reason: &str, probe: &mut SdProbeDriver) -> SdResultCode {
    queue_line!(
        "sdprobe: data_spi_mhz={}",
        SdProbeDriver::data_spi_rate_mhz()
    );
    match probe.probe().await {
        Ok(status) => {
            publish_probe_status(reason, status).await;
            SdResultCode::Ok
        }
        Err(err) => {
            probe.recover_after_timeout();
            publish_probe_error(reason, err).await;
            SdResultCode::OperationFailed
        }
    }
}

async fn publish_probe_status(reason: &str, status: SdProbeStatus) {
    let version = match status.version {
        SdCardVersion::V1 => "v1.x",
        SdCardVersion::V2 => "v2+",
    };
    let capacity = if status.high_capacity {
        "sdhc_or_sdxc"
    } else {
        "sdsc"
    };
    let filesystem = match status.filesystem {
        SdFilesystem::ExFat => "exfat",
        SdFilesystem::Fat32 => "fat32",
        SdFilesystem::Fat16 => "fat16",
        SdFilesystem::Fat12 => "fat12",
        SdFilesystem::Ntfs => "ntfs",
        SdFilesystem::Unknown => "unknown",
    };
    let gib_x100 = status
        .capacity_bytes
        .saturating_mul(100)
        .saturating_div(1024 * 1024 * 1024);
    queue_line!(
        "sdprobe[{}]: card_detected version={} capacity={} fs={} bytes={} size_gib={}.{:02}",
        reason,
        version,
        capacity,
        filesystem,
        status.capacity_bytes,
        gib_x100 / 100,
        gib_x100 % 100
    );
}

async fn publish_probe_error(reason: &str, err: SdProbeError) {
    match err {
        SdProbeError::Spi(err) => {
            queue_line!("sdprobe[{}]: not_detected spi_error={:?}", reason, err)
        }
        SdProbeError::SpiConfig(err) => {
            queue_line!("sdprobe[{}]: not_detected spi_config_error={:?}", reason, err)
        }
        SdProbeError::Cmd0Failed(r1) => {
            queue_line!("sdprobe[{}]: not_detected cmd0_r1=0x{:02x}", reason, r1)
        }
        SdProbeError::Cmd8Unexpected(r1) => {
            queue_line!("sdprobe[{}]: not_detected cmd8_r1=0x{:02x}", reason, r1)
        }
        SdProbeError::Cmd8EchoMismatch(r7) => queue_line!(
            "sdprobe[{}]: not_detected cmd8_echo={:02x}{:02x}{:02x}{:02x}",
            reason,
            r7[0],
            r7[1],
            r7[2],
            r7[3]
        ),
        SdProbeError::Acmd41Timeout(r1) => queue_line!(
            "sdprobe[{}]: not_detected acmd41_last_r1=0x{:02x}",
            reason,
            r1
        ),
        SdProbeError::Cmd58Unexpected(r1) => {
            queue_line!("sdprobe[{}]: not_detected cmd58_r1=0x{:02x}", reason, r1)
        }
        SdProbeError::Cmd9Unexpected(r1) => {
            queue_line!("sdprobe[{}]: not_detected cmd9_r1=0x{:02x}", reason, r1)
        }
        SdProbeError::Cmd16Unexpected(r1) => {
            queue_line!("sdprobe[{}]: not_detected cmd16_r1=0x{:02x}", reason, r1)
        }
        SdProbeError::Cmd17Unexpected(r1) => {
            queue_line!("sdprobe[{}]: not_detected cmd17_r1=0x{:02x}", reason, r1)
        }
        SdProbeError::Cmd24Unexpected(r1) => {
            queue_line!("sdprobe[{}]: not_detected cmd24_r1=0x{:02x}", reason, r1)
        }
        SdProbeError::Cmd25Unexpected(r1) => {
            queue_line!("sdprobe[{}]: not_detected cmd25_r1=0x{:02x}", reason, r1)
        }
        SdProbeError::Cmd13Unexpected(r1, status) => queue_line!(
            "sdprobe[{}]: not_detected cmd13_r1=0x{:02x} status=0x{:02x}",
            reason,
            r1,
            status
        ),
        SdProbeError::NoResponse(cmd) => {
            queue_line!("sdprobe[{}]: not_detected cmd{}_no_response", reason, cmd)
        }
        SdProbeError::DataTokenTimeout(cmd) => queue_line!(
            "sdprobe[{}]: not_detected cmd{}_data_token_timeout",
            reason,
            cmd
        ),
        SdProbeError::DataTokenUnexpected(cmd, token) => queue_line!(
            "sdprobe[{}]: not_detected cmd{}_data_token=0x{:02x}",
            reason,
            cmd,
            token
        ),
        SdProbeError::WriteDataRejected(response) => queue_line!(
            "sdprobe[{}]: not_detected write_response=0x{:02x}",
            reason,
            response
        ),
        SdProbeError::DmaTransferTimeout => {
            queue_line!("sdprobe[{}]: not_detected dma_transfer_timeout", reason)
        }
        SdProbeError::WriteBusyTimeout { elapsed_ms, polls } => queue_line!(
            "sdprobe[{}]: not_detected write_busy_timeout elapsed_ms={} polls={}",
            reason,
            elapsed_ms,
            polls
        ),
        SdProbeError::WriteLengthInvalid(len) => queue_line!(
            "sdprobe[{}]: not_detected write_len_invalid={}",
            reason,
            len
        ),
        SdProbeError::NotInitialized => {
            queue_line!("sdprobe[{}]: not_detected not_initialized", reason)
        }
        SdProbeError::CapacityDecodeFailed => {
            queue_line!("sdprobe[{}]: not_detected capacity_decode_failed", reason)
        }
    }
}

pub(super) async fn run_rw_verify(
    reason: &str,
    lba: u32,
    probe: &mut SdProbeDriver,
) -> SdResultCode {
    if lba == 0 {
        queue_line!("sdrw[{}]: refused_lba0", reason);
        return SdResultCode::RefusedLba0;
    }
    if let Err(err) = probe.init().await {
        probe.recover_after_timeout();
        queue_line!("sdrw[{}]: init_error={:?}", reason, err);
        return SdResultCode::InitFailed;
    }

    let mut before = [0u8; SD_SECTOR_SIZE];
    if let Err(code) = read_sector(reason, "read_before", lba, probe, &mut before).await {
        return code;
    }
    match probe.write_sector(lba, &before).await {
        Ok(()) => {}
        Err(err) if err.is_timeout() => {
            probe.recover_after_timeout();
            queue_line!("sdrw[{}]: write_timeout lba={}", reason, lba);
            return SdResultCode::OperationFailed;
        }
        Err(err) => {
            queue_line!("sdrw[{}]: write_error lba={} err={:?}", reason, lba, err);
            return SdResultCode::OperationFailed;
        }
    }

    let mut after = [0u8; SD_SECTOR_SIZE];
    if let Err(code) = read_sector(reason, "read_after", lba, probe, &mut after).await {
        return code;
    }
    if let Some(index) = before.iter().zip(after.iter()).position(|(a, b)| a != b) {
        queue_line!(
            "sdrw[{}]: verify_mismatch lba={} byte={} before=0x{:02x} after=0x{:02x}",
            reason,
            lba,
            index,
            before[index],
            after[index]
        );
        SdResultCode::VerifyMismatch
    } else {
        queue_line!(
            "sdrw[{}]: verify_ok lba={} bytes={}",
            reason,
            lba,
            SD_SECTOR_SIZE
        );
        SdResultCode::Ok
    }
}

async fn read_sector(
    reason: &str,
    phase: &str,
    lba: u32,
    probe: &mut SdProbeDriver,
    output: &mut [u8; SD_SECTOR_SIZE],
) -> Result<(), SdResultCode> {
    match probe.read_sector(lba, output).await {
        Ok(()) => Ok(()),
        Err(err) if err.is_timeout() => {
            probe.recover_after_timeout();
            queue_line!("sdrw[{}]: {}_timeout lba={}", reason, phase, lba);
            Err(SdResultCode::OperationFailed)
        }
        Err(err) => {
            queue_line!("sdrw[{}]: {}_error lba={} err={:?}", reason, phase, lba, err);
            Err(SdResultCode::OperationFailed)
        }
    }
}
