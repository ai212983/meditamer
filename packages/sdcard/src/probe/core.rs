use embassy_time::Timer;
use embedded_hal::spi::SpiBus;
use esp_hal::{
    gpio::Output,
    spi::{
        master::{Config as SpiConfig, ConfigError as SpiConfigError, Spi},
        Error as SpiError,
        Mode as SpiMode,
    },
    time::Rate,
    Blocking,
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
    spi: Spi<'d, Blocking>,
    cs: Output<'d>,
    high_capacity: Option<bool>,
    cached_sector_lba: Option<u32>,
    cached_sector: [u8; SD_SECTOR_SIZE],
    next_free_cluster_hint: Option<u32>,
}

impl<'d> SdCardProbe<'d> {
    pub fn new(spi: Spi<'d, Blocking>, mut cs: Output<'d>) -> Self {
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

    pub fn is_initialized(&self) -> bool {
        self.high_capacity.is_some()
    }

    pub fn invalidate(&mut self) {
        self.high_capacity = None;
        self.cached_sector_lba = None;
        self.next_free_cluster_hint = None;
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
}
