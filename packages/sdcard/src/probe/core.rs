use embassy_embedded_hal::SetConfig;
use embassy_time::{with_timeout, Duration, Instant, Timer};
use embedded_hal_async::spi::SpiBus;
use esp_hal::spi::master::SpiDmaBus;
use esp_hal::{
    gpio::Output,
    spi::{
        master::{Config as SpiConfig, ConfigError as SpiConfigError},
        Error as SpiError, Mode as SpiMode,
    },
    time::Rate,
    Async,
};
pub type DefaultSdSpi<'d> = SpiDmaBus<'d, Async>;

const SD_DMA_TRANSFER_DEADLINE_MS: u64 = 250;
#[allow(async_fn_in_trait)]
pub trait SdSpiBus:
    SpiBus<u8, Error = SpiError> + SetConfig<Config = SpiConfig, ConfigError = SpiConfigError>
{
    async fn transfer_in_place_with_deadline(
        &mut self,
        words: &mut [u8],
    ) -> Result<(), SdProbeError> {
        match with_timeout(
            Duration::from_millis(SD_DMA_TRANSFER_DEADLINE_MS),
            SpiBus::transfer_in_place(self, words),
        )
        .await
        {
            Ok(result) => result.map_err(SdProbeError::from),
            Err(_) => Err(SdProbeError::DmaTransferTimeout),
        }
    }
}

impl<T> SdSpiBus for T where
    T: SpiBus<u8, Error = SpiError> + SetConfig<Config = SpiConfig, ConfigError = SpiConfigError>
{
}

const SD_CMD0: u8 = 0;
const SD_CMD8: u8 = 8;
const SD_CMD9: u8 = 9;
const SD_CMD13: u8 = 13;
const SD_CMD16: u8 = 16;
const SD_CMD17: u8 = 17;
const SD_CMD24: u8 = 24;
const SD_CMD25: u8 = 25;
const SD_CMD55: u8 = 55;
const SD_ACMD23: u8 = 23;
const SD_ACMD41: u8 = 41;
const SD_CMD58: u8 = 58;
const SD_INIT_SPI_RATE_KHZ: u32 = 400;
const SD_DATA_SPI_RATE_MHZ_DEFAULT: u32 = 36;
const SD_DATA_SPI_RATE_MHZ_MIN: u32 = 12;
const SD_DATA_SPI_RATE_MHZ_MAX: u32 = 40;
pub const SD_SECTOR_SIZE: usize = 512;
const SD_WRITE_PREAMBLE_BYTES: usize = 2;
const SD_WRITE_FINISH_BYTES: usize = 32;
const SD_WRITE_FRAME_BYTES: usize =
    SD_WRITE_PREAMBLE_BYTES + SD_SECTOR_SIZE + SD_WRITE_FINISH_BYTES;
/// DMA capacity needed to keep one complete SD write frame in one transfer.
/// The 546-byte protocol frame is rounded up to the DMA alignment boundary.
pub const SD_DMA_BUFFER_SIZE: usize = (SD_WRITE_FRAME_BYTES + 3) & !3;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SdWriteMetrics {
    pub cmd24_sectors: u32,
    pub cmd25_attempt_bursts: u32,
    pub cmd25_attempt_sectors: u32,
    pub cmd25_success_bursts: u32,
    pub cmd25_success_sectors: u32,
    pub cmd25_fallback_bursts: u32,
    pub cmd25_success_burst_ms_total: u32,
    pub cmd25_ready_wait_count: u32,
    pub cmd25_ready_wait_ms_total: u32,
    pub cmd25_ready_wait_polls_total: u32,
    pub cmd25_ready_wait_over_1ms: u32,
    pub cmd25_ready_wait_over_4ms: u32,
    pub cmd25_ready_wait_over_8ms: u32,
    pub acmd23_attempts: u32,
    pub acmd23_successes: u32,
    pub acmd23_unsupported: u32,
}

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
    Cmd25Unexpected(u8),
    Cmd13Unexpected(u8, u8),
    NoResponse(u8),
    DataTokenTimeout(u8),
    DataTokenUnexpected(u8, u8),
    WriteDataRejected(u8),
    DmaTransferTimeout,
    WriteBusyTimeout { elapsed_ms: u32, polls: u32 },
    WriteLengthInvalid(usize),
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

impl SdProbeError {
    pub fn is_timeout(&self) -> bool {
        matches!(
            self,
            Self::DmaTransferTimeout
                | Self::DataTokenTimeout(_)
                | Self::WriteBusyTimeout { .. }
        )
    }

    pub fn requires_bus_recovery(&self) -> bool {
        matches!(self, Self::Spi(_)) || self.is_timeout()
    }
}

pub struct SdCardProbe<'d, SPI = DefaultSdSpi<'d>> {
    spi: SPI,
    cs: Output<'d>,
    high_capacity: Option<bool>,
    cached_sector_lba: Option<u32>,
    cached_sector: [u8; SD_WRITE_FRAME_BYTES],
    next_free_cluster_hint: Option<u32>,
    write_metrics: SdWriteMetrics,
}

impl<'d, SPI> SdCardProbe<'d, SPI>
where
    SPI: SdSpiBus,
{
    pub fn new(spi: SPI, mut cs: Output<'d>) -> Self {
        cs.set_high();
        Self {
            spi,
            cs,
            high_capacity: None,
            cached_sector_lba: None,
            cached_sector: [0; SD_WRITE_FRAME_BYTES],
            next_free_cluster_hint: None,
            write_metrics: SdWriteMetrics::default(),
        }
    }

    pub fn is_initialized(&self) -> bool {
        self.high_capacity.is_some()
    }

    pub fn invalidate(&mut self) {
        self.high_capacity = None;
        self.cached_sector_lba = None;
        self.next_free_cluster_hint = None;
    }

    /// Restores the bus to an idle state after a cancelled DMA future.
    pub fn recover_after_timeout(&mut self) {
        self.cs.set_high();
        self.invalidate();
    }

    pub fn write_metrics_snapshot(&self) -> SdWriteMetrics {
        self.write_metrics
    }

    pub(crate) fn record_cmd24_sector_write(&mut self) {
        self.write_metrics.cmd24_sectors = self.write_metrics.cmd24_sectors.saturating_add(1);
    }

    pub(crate) fn record_cmd25_attempt(&mut self, sectors: usize) {
        self.write_metrics.cmd25_attempt_bursts =
            self.write_metrics.cmd25_attempt_bursts.saturating_add(1);
        self.write_metrics.cmd25_attempt_sectors = self
            .write_metrics
            .cmd25_attempt_sectors
            .saturating_add(sectors.min(u32::MAX as usize) as u32);
    }

    pub(crate) fn record_cmd25_success(&mut self, sectors: usize, burst_elapsed_ms: u32) {
        self.write_metrics.cmd25_success_bursts =
            self.write_metrics.cmd25_success_bursts.saturating_add(1);
        self.write_metrics.cmd25_success_sectors = self
            .write_metrics
            .cmd25_success_sectors
            .saturating_add(sectors.min(u32::MAX as usize) as u32);
        self.write_metrics.cmd25_success_burst_ms_total = self
            .write_metrics
            .cmd25_success_burst_ms_total
            .saturating_add(burst_elapsed_ms);
    }

    pub(crate) fn record_cmd25_fallback(&mut self) {
        self.write_metrics.cmd25_fallback_bursts =
            self.write_metrics.cmd25_fallback_bursts.saturating_add(1);
    }

    pub(crate) fn record_cmd25_ready_wait(&mut self, elapsed_ms: u32, polls: u32) {
        self.write_metrics.cmd25_ready_wait_count =
            self.write_metrics.cmd25_ready_wait_count.saturating_add(1);
        self.write_metrics.cmd25_ready_wait_ms_total = self
            .write_metrics
            .cmd25_ready_wait_ms_total
            .saturating_add(elapsed_ms);
        self.write_metrics.cmd25_ready_wait_polls_total = self
            .write_metrics
            .cmd25_ready_wait_polls_total
            .saturating_add(polls);
        if elapsed_ms >= 1 {
            self.write_metrics.cmd25_ready_wait_over_1ms = self
                .write_metrics
                .cmd25_ready_wait_over_1ms
                .saturating_add(1);
        }
        if elapsed_ms >= 4 {
            self.write_metrics.cmd25_ready_wait_over_4ms = self
                .write_metrics
                .cmd25_ready_wait_over_4ms
                .saturating_add(1);
        }
        if elapsed_ms >= 8 {
            self.write_metrics.cmd25_ready_wait_over_8ms = self
                .write_metrics
                .cmd25_ready_wait_over_8ms
                .saturating_add(1);
        }
    }

    pub(crate) fn record_acmd23_attempt(&mut self) {
        self.write_metrics.acmd23_attempts = self.write_metrics.acmd23_attempts.saturating_add(1);
    }

    pub(crate) fn record_acmd23_result(&mut self, supported: bool) {
        if supported {
            self.write_metrics.acmd23_successes =
                self.write_metrics.acmd23_successes.saturating_add(1);
        } else {
            self.write_metrics.acmd23_unsupported =
                self.write_metrics.acmd23_unsupported.saturating_add(1);
        }
    }

    pub fn data_spi_rate_mhz() -> u32 {
        let configured = option_env!("MEDITAMER_SD_SPI_DATA_MHZ")
            .or(option_env!("SD_SPI_DATA_MHZ"))
            .and_then(parse_ascii_u32);
        match configured {
            Some(mhz) if (SD_DATA_SPI_RATE_MHZ_MIN..=SD_DATA_SPI_RATE_MHZ_MAX).contains(&mhz) => {
                mhz
            }
            _ => SD_DATA_SPI_RATE_MHZ_DEFAULT,
        }
    }
}
