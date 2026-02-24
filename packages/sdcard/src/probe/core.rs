use embassy_time::Timer;
use esp_hal::{
    gpio::Output,
    spi::{
        master::{Config as SpiConfig, ConfigError as SpiConfigError, Spi},
        Error as SpiError,
    },
    time::Rate,
    Async,
};

const SD_CMD0: u8 = 0;
const SD_CMD8: u8 = 8;
const SD_CMD9: u8 = 9;
const SD_CMD16: u8 = 16;
const SD_CMD17: u8 = 17;
const SD_CMD24: u8 = 24;
const SD_CMD55: u8 = 55;
const SD_ACMD41: u8 = 41;
const SD_CMD58: u8 = 58;
const SD_INIT_SPI_RATE_KHZ: u32 = 400;
const SD_DATA_SPI_RATE_MHZ: u32 = 24;
pub const SD_SECTOR_SIZE: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdCardVersion {
    V1,
    V2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SdProbeStatus {
    pub version: SdCardVersion,
    pub high_capacity: bool,
    pub capacity_bytes: u64,
    pub filesystem: SdFilesystem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdFilesystem {
    ExFat,
    Fat32,
    Fat16,
    Fat12,
    Ntfs,
    Unknown,
}

#[derive(Debug)]
pub enum SdProbeError {
    Spi(SpiError),
    SpiConfig(SpiConfigError),
    Cmd0Failed(u8),
    Cmd8Unexpected(u8),
    Cmd8EchoMismatch([u8; 4]),
    Acmd41Timeout(u8),
    Cmd58Unexpected(u8),
    Cmd9Unexpected(u8),
    Cmd16Unexpected(u8),
    Cmd17Unexpected(u8),
    Cmd24Unexpected(u8),
    NoResponse(u8),
    DataTokenTimeout(u8),
    DataTokenUnexpected(u8, u8),
    WriteDataRejected(u8),
    WriteBusyTimeout,
    NotInitialized,
    CapacityDecodeFailed,
}

impl From<SpiError> for SdProbeError {
    fn from(value: SpiError) -> Self {
        Self::Spi(value)
    }
}

impl From<SpiConfigError> for SdProbeError {
    fn from(value: SpiConfigError) -> Self {
        Self::SpiConfig(value)
    }
}

pub struct SdCardProbe<'d> {
    spi: Spi<'d, Async>,
    cs: Output<'d>,
    high_capacity: Option<bool>,
    cached_sector_lba: Option<u32>,
    cached_sector: [u8; SD_SECTOR_SIZE],
    next_free_cluster_hint: Option<u32>,
}

impl<'d> SdCardProbe<'d> {
    pub fn new(spi: Spi<'d, Async>, mut cs: Output<'d>) -> Self {
        cs.set_high();
        Self {
            spi,
            cs,
            high_capacity: None,
            cached_sector_lba: None,
            cached_sector: [0; SD_SECTOR_SIZE],
            next_free_cluster_hint: None,
        }
    }

    pub async fn init(&mut self) -> Result<SdProbeStatus, SdProbeError> {
        self.cached_sector_lba = None;
        self.probe().await
    }

    pub(crate) fn next_free_cluster_hint(&self) -> Option<u32> {
        self.next_free_cluster_hint
    }

    pub(crate) fn set_next_free_cluster_hint(&mut self, cluster: u32) {
        self.next_free_cluster_hint = Some(cluster);
    }

    pub(crate) fn lower_next_free_cluster_hint(&mut self, cluster: u32) {
        if cluster < 2 {
            return;
        }
        if let Some(current) = self.next_free_cluster_hint {
            if current <= cluster {
                return;
            }
        }
        self.next_free_cluster_hint = Some(cluster);
    }

    pub async fn read_sector(
        &mut self,
        lba: u32,
        out: &mut [u8; SD_SECTOR_SIZE],
    ) -> Result<(), SdProbeError> {
        if self.cached_sector_lba == Some(lba) {
            out.copy_from_slice(&self.cached_sector);
            return Ok(());
        }
        let high_capacity = self.high_capacity.ok_or(SdProbeError::NotInitialized)?;
        self.read_data_sector_512_into(lba, high_capacity, out)
            .await?;
        self.cached_sector.copy_from_slice(out);
        self.cached_sector_lba = Some(lba);
        Ok(())
    }

    pub async fn write_sector(
        &mut self,
        lba: u32,
        data: &[u8; SD_SECTOR_SIZE],
    ) -> Result<(), SdProbeError> {
        let high_capacity = self.high_capacity.ok_or(SdProbeError::NotInitialized)?;
        let arg = if high_capacity {
            lba
        } else {
            lba.saturating_mul(SD_SECTOR_SIZE as u32)
        };

        let cmd24_r1 = self
            .send_command_hold_cs(SD_CMD24, arg, 0xFF, &mut [])
            .await?;
        if cmd24_r1 != 0x00 {
            self.end_transaction().await;
            return Err(SdProbeError::Cmd24Unexpected(cmd24_r1));
        }

        let _ = self.transfer_byte(0xFF).await?;
        let _ = self.transfer_byte(0xFE).await?;
        for &byte in data.iter() {
            let _ = self.transfer_byte(byte).await?;
        }
        // Data CRC16 is ignored in SPI mode unless CRC is explicitly enabled.
        let _ = self.transfer_byte(0xFF).await?;
        let _ = self.transfer_byte(0xFF).await?;

        let response = self.transfer_byte(0xFF).await? & 0x1F;
        if response != 0x05 {
            self.end_transaction().await;
            return Err(SdProbeError::WriteDataRejected(response));
        }

        let mut released = false;
        for _ in 0..200_000 {
            if self.transfer_byte(0xFF).await? == 0xFF {
                released = true;
                break;
            }
        }
        self.end_transaction().await;
        if !released {
            return Err(SdProbeError::WriteBusyTimeout);
        }
        self.cached_sector.copy_from_slice(data);
        self.cached_sector_lba = Some(lba);
        Ok(())
    }

    pub async fn probe(&mut self) -> Result<SdProbeStatus, SdProbeError> {
        self.apply_init_clock().await?;
        self.cs.set_high();
        self.send_dummy_clocks(10).await?;

        let mut cmd0_r1 = 0xFFu8;
        for _ in 0..16 {
            cmd0_r1 = self.send_command(SD_CMD0, 0, 0x95, &mut []).await?;
            if cmd0_r1 == 0x01 {
                break;
            }
        }
        if cmd0_r1 != 0x01 {
            return Err(SdProbeError::Cmd0Failed(cmd0_r1));
        }

        let mut r7 = [0u8; 4];
        let cmd8_r1 = self
            .send_command(SD_CMD8, 0x0000_01AA, 0x87, &mut r7)
            .await?;
        let card_version = if cmd8_r1 == 0x01 {
            if r7[2] != 0x01 || r7[3] != 0xAA {
                return Err(SdProbeError::Cmd8EchoMismatch(r7));
            }
            SdCardVersion::V2
        } else if (cmd8_r1 & 0x04) != 0 {
            SdCardVersion::V1
        } else {
            return Err(SdProbeError::Cmd8Unexpected(cmd8_r1));
        };

        let acmd41_arg = if card_version == SdCardVersion::V2 {
            0x4000_0000
        } else {
            0
        };
        let mut acmd41_r1 = 0xFFu8;
        let mut acmd41_ok = false;
        for _ in 0..200 {
            let _ = self.send_command(SD_CMD55, 0, 0x65, &mut []).await?;
            acmd41_r1 = self
                .send_command(SD_ACMD41, acmd41_arg, 0x77, &mut [])
                .await?;
            if acmd41_r1 == 0x00 {
                acmd41_ok = true;
                break;
            }
            self.retry_delay().await;
        }
        if !acmd41_ok {
            return Err(SdProbeError::Acmd41Timeout(acmd41_r1));
        }

        if card_version == SdCardVersion::V1 {
            let cmd16_r1 = self
                .send_command(SD_CMD16, SD_SECTOR_SIZE as u32, 0xFF, &mut [])
                .await?;
            if cmd16_r1 != 0x00 {
                return Err(SdProbeError::Cmd16Unexpected(cmd16_r1));
            }
        }

        self.apply_data_clock().await?;

        let mut ocr = [0u8; 4];
        let cmd58_r1 = self.send_command(SD_CMD58, 0, 0xFD, &mut ocr).await?;
        if cmd58_r1 != 0x00 {
            return Err(SdProbeError::Cmd58Unexpected(cmd58_r1));
        }

        let cmd9_r1 = self.send_command_hold_cs(SD_CMD9, 0, 0xAF, &mut []).await?;
        if cmd9_r1 != 0x00 {
            self.end_transaction().await;
            return Err(SdProbeError::Cmd9Unexpected(cmd9_r1));
        }
        let csd = self.read_data_block().await?;
        self.end_transaction().await;
        let capacity_bytes =
            decode_capacity_bytes(&csd).ok_or(SdProbeError::CapacityDecodeFailed)?;
        let high_capacity = (ocr[0] & 0x40) != 0;
        let filesystem = self.detect_filesystem(high_capacity).await?;

        let status = SdProbeStatus {
            version: card_version,
            high_capacity,
            capacity_bytes,
            filesystem,
        };
        self.high_capacity = Some(high_capacity);
        Ok(status)
    }

    async fn apply_init_clock(&mut self) -> Result<(), SdProbeError> {
        let config = SpiConfig::default().with_frequency(Rate::from_khz(SD_INIT_SPI_RATE_KHZ));
        self.spi.apply_config(&config)?;
        Ok(())
    }

    async fn apply_data_clock(&mut self) -> Result<(), SdProbeError> {
        let config = SpiConfig::default().with_frequency(Rate::from_mhz(SD_DATA_SPI_RATE_MHZ));
        self.spi.apply_config(&config)?;
        Ok(())
    }

    async fn send_command(
        &mut self,
        cmd: u8,
        arg: u32,
        crc: u8,
        extra_response: &mut [u8],
    ) -> Result<u8, SdProbeError> {
        self.send_command_inner(cmd, arg, crc, extra_response, true)
            .await
    }

    async fn send_command_hold_cs(
        &mut self,
        cmd: u8,
        arg: u32,
        crc: u8,
        extra_response: &mut [u8],
    ) -> Result<u8, SdProbeError> {
        self.send_command_inner(cmd, arg, crc, extra_response, false)
            .await
    }

    async fn send_command_inner(
        &mut self,
        cmd: u8,
        arg: u32,
        crc: u8,
        extra_response: &mut [u8],
        release_cs_after: bool,
    ) -> Result<u8, SdProbeError> {
        let frame = [
            0x40 | cmd,
            (arg >> 24) as u8,
            (arg >> 16) as u8,
            (arg >> 8) as u8,
            arg as u8,
            crc,
        ];

        self.cs.set_low();
        for byte in frame {
            let _ = self.transfer_byte(byte).await?;
        }

        let mut r1 = 0xFFu8;
        let mut got_response = false;
        for _ in 0..16 {
            r1 = self.transfer_byte(0xFF).await?;
            if (r1 & 0x80) == 0 {
                got_response = true;
                break;
            }
        }

        if !got_response {
            self.end_transaction().await;
            return Err(SdProbeError::NoResponse(cmd));
        }

        for slot in extra_response.iter_mut() {
            *slot = self.transfer_byte(0xFF).await?;
        }

        if release_cs_after {
            self.end_transaction().await;
        }
        Ok(r1)
    }

    async fn send_dummy_clocks(&mut self, bytes: usize) -> Result<(), SdProbeError> {
        for _ in 0..bytes {
            let _ = self.transfer_byte(0xFF).await?;
        }
        Ok(())
    }

    async fn transfer_byte(&mut self, byte: u8) -> Result<u8, SdProbeError> {
        let mut frame = [byte];
        self.spi.transfer_in_place_async(&mut frame).await?;
        Ok(frame[0])
    }

    async fn read_data_block(&mut self) -> Result<[u8; 16], SdProbeError> {
        let mut token = 0xFFu8;
        let mut got_token = false;
        for _ in 0..50_000 {
            token = self.transfer_byte(0xFF).await?;
            if token != 0xFF {
                got_token = true;
                break;
            }
        }
        if !got_token {
            return Err(SdProbeError::DataTokenTimeout(SD_CMD9));
        }
        if token != 0xFE {
            return Err(SdProbeError::DataTokenUnexpected(SD_CMD9, token));
        }

        let mut block = [0u8; 16];
        for slot in block.iter_mut() {
            *slot = self.transfer_byte(0xFF).await?;
        }
        // Read and discard CRC16.
        let _ = self.transfer_byte(0xFF).await?;
        let _ = self.transfer_byte(0xFF).await?;
        Ok(block)
    }

    async fn read_data_sector_512_into(
        &mut self,
        lba: u32,
        high_capacity: bool,
        out: &mut [u8; SD_SECTOR_SIZE],
    ) -> Result<(), SdProbeError> {
        let arg = if high_capacity {
            lba
        } else {
            lba.saturating_mul(512)
        };
        let cmd17_r1 = self
            .send_command_hold_cs(SD_CMD17, arg, 0xFF, &mut [])
            .await?;
        if cmd17_r1 != 0x00 {
            self.end_transaction().await;
            return Err(SdProbeError::Cmd17Unexpected(cmd17_r1));
        }

        let mut token = 0xFFu8;
        let mut got_token = false;
        for _ in 0..50_000 {
            token = self.transfer_byte(0xFF).await?;
            if token != 0xFF {
                got_token = true;
                break;
            }
        }
        if !got_token {
            self.end_transaction().await;
            return Err(SdProbeError::DataTokenTimeout(SD_CMD17));
        }
        if token != 0xFE {
            self.end_transaction().await;
            return Err(SdProbeError::DataTokenUnexpected(SD_CMD17, token));
        }

        for slot in out.iter_mut() {
            *slot = self.transfer_byte(0xFF).await?;
        }
        // Discard data CRC16.
        let _ = self.transfer_byte(0xFF).await?;
        let _ = self.transfer_byte(0xFF).await?;
        self.end_transaction().await;
        Ok(())
    }

    async fn retry_delay(&self) {
        Timer::after_millis(1).await;
    }

    async fn end_transaction(&mut self) {
        self.cs.set_high();
        let _ = self.transfer_byte(0xFF).await;
    }

    async fn detect_filesystem(
        &mut self,
        high_capacity: bool,
    ) -> Result<SdFilesystem, SdProbeError> {
        let mut sector = [0u8; SD_SECTOR_SIZE];
        self.read_data_sector_512_into(0, high_capacity, &mut sector)
            .await?;
        if let Some(fs) = detect_vbr_filesystem(&sector) {
            return Ok(fs);
        }

        let mut partition_type = 0u8;
        let mut partition_lba = 0u32;
        for idx in 0..4usize {
            let off = 446 + idx * 16;
            let p_type = sector[off + 4];
            let start = u32::from_le_bytes([
                sector[off + 8],
                sector[off + 9],
                sector[off + 10],
                sector[off + 11],
            ]);
            if p_type != 0 && start != 0 {
                partition_type = p_type;
                partition_lba = start;
                break;
            }
        }

        if partition_lba == 0 {
            return Ok(SdFilesystem::Unknown);
        }

        if partition_type == 0xEE {
            // Protective MBR (GPT). Read the first GPT partition entry.
            self.read_data_sector_512_into(2, high_capacity, &mut sector)
                .await?;
            let start = u64::from_le_bytes([
                sector[32], sector[33], sector[34], sector[35], sector[36], sector[37], sector[38],
                sector[39],
            ]);
            if start != 0 && start <= u32::MAX as u64 {
                partition_lba = start as u32;
            }
        }

        self.read_data_sector_512_into(partition_lba, high_capacity, &mut sector)
            .await?;
        Ok(detect_vbr_filesystem(&sector).unwrap_or(SdFilesystem::Unknown))
    }
}
