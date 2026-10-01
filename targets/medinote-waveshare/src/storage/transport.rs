//! Production native SDMMC sector transport.

use aligned::{Aligned, A4};
use block_device_driver::BlockDevice as _;
use core::cell::RefCell;
use embassy_time::Delay;
use embassy_time::{with_timeout, Duration, Instant};
use esp_hal::{sdmmc::Slot, Async};

use crate::sd_recovery;
use crate::storage::bus::BorrowedBus;
use sdcard::transport::{SectorTransport, TransportError, TransportErrorKind};

const ENUMERATE_MS: u64 = 5_000;
const IO_MS: u64 = 2_000;
const CLOCK_HZ: u32 = 20_000_000;
type Card = sdio::BlockDevice<sdio::sd::Card, BorrowedBus, Delay, 512>;

/// Native SDMMC transport with one DMA-aligned staging sector.
pub struct NativeTransport {
    card: Option<Card>,
    pub(crate) bus: &'static RefCell<Slot<'static, 1, Async>>,
    workspace: &'static mut Aligned<A4, [u8; 512]>,
    capacity: u64,
}

impl NativeTransport {
    pub fn new(
        bus: &'static RefCell<Slot<'static, 1, Async>>,
        workspace: &'static mut Aligned<A4, [u8; 512]>,
    ) -> Self {
        Self {
            card: None,
            bus,
            workspace,
            capacity: 0,
        }
    }

    pub async fn initialize(&mut self) -> Result<(), TransportError> {
        self.recover().await
    }

    pub async fn recover(&mut self) -> Result<(), TransportError> {
        // Dropping the previous card first releases all bus borrows and HAL
        // state before reset and construction of the fresh BlockDevice.
        self.card = None;
        self.capacity = 0;
        {
            let mut slot = self.bus.borrow_mut();
            sd_recovery::reset(&mut slot).map_err(|_| unavailable("sd reset failed"))?;
        }
        self.reacquire().await
    }

    pub(crate) fn quiesce_for_suspend(&mut self) {
        self.card = None;
        self.capacity = 0;
        if sd_recovery::quiesce().is_err() {
            halt();
        }
    }

    async fn reacquire(&mut self) -> Result<(), TransportError> {
        {
            let mut slot = self.bus.borrow_mut();
            slot.set_bus_low_level(esp_hal::sdmmc::BusWidth::Bit1, 400_000)
                .map_err(|_| unavailable("sd idle bus configuration failed"))?;
        }
        let mut card = Card::new_uninit_sd_card(BorrowedBus::new(self.bus), Delay);
        Self::run_timed(ENUMERATE_MS, card.reacquire(CLOCK_HZ)).await?;
        let capacity = card.card().csd.block_count();
        if capacity < 2
            || capacity > u64::from(u32::MAX)
            || (!card.card().ocr.high_capacity() && capacity > u64::from(u32::MAX) / 512 + 1)
        {
            return Err(TransportError::new(
                TransportErrorKind::Bounds,
                "unsupported SD capacity",
                None,
            ));
        }
        console::println!(
            "STORAGE_CARD sectors={} high_capacity={}",
            capacity,
            card.card().ocr.high_capacity()
        );
        self.capacity = capacity;
        self.card = Some(card);
        Ok(())
    }

    async fn run_timed<T>(
        millis: u64,
        future: impl core::future::Future<Output = Result<T, sdio::MmcError>>,
    ) -> Result<T, TransportError> {
        let start = Instant::now();
        let result = with_timeout(Duration::from_millis(millis), future).await;
        // The HAL future is dropped by the time this scope ends. Always quiesce
        // before making the DMA workspace available again.
        let mut result = match result {
            Ok(value) => value.map_err(map_error),
            Err(_) => Err(timeout()),
        };
        if start.elapsed() > Duration::from_millis(millis) {
            result = Err(timeout());
        }
        if sd_recovery::quiesce().is_err() {
            halt();
        }
        result
    }

    fn bounds(&self, lba: u32) -> Result<(), TransportError> {
        if self.card.is_none() {
            return Err(unavailable("SD card not initialized"));
        }
        if u64::from(lba) >= self.capacity {
            Err(TransportError::new(
                TransportErrorKind::Bounds,
                "sector outside SD capacity",
                Some(lba),
            ))
        } else {
            Ok(())
        }
    }
}

impl SectorTransport for NativeTransport {
    type Error = TransportError;

    async fn read_sector(&mut self, lba: u32, sector: &mut [u8; 512]) -> Result<(), Self::Error> {
        self.bounds(lba)?;
        let workspace = &mut *self.workspace;
        let card = self
            .card
            .as_mut()
            .ok_or_else(|| unavailable("SD card not initialized"))?;
        let result = Self::run_timed(IO_MS, card.read(lba, core::slice::from_mut(workspace))).await;
        if result.is_ok() {
            sector.copy_from_slice(&workspace[..]);
        }
        result
    }

    async fn write_sector(&mut self, lba: u32, sector: &[u8; 512]) -> Result<(), Self::Error> {
        self.bounds(lba)?;
        self.workspace.copy_from_slice(sector);
        let card = self
            .card
            .as_mut()
            .ok_or_else(|| unavailable("SD card not initialized"))?;
        Self::run_timed(
            IO_MS,
            card.write(lba, core::slice::from_ref(&*self.workspace)),
        )
        .await
    }

    async fn write_sectors_contiguous(
        &mut self,
        start_lba: u32,
        sectors: &[u8],
    ) -> Result<(), Self::Error> {
        if self.card.is_none() {
            return Err(unavailable("SD card not initialized"));
        }
        if !sectors.len().is_multiple_of(512) {
            return Err(TransportError::new(
                TransportErrorKind::Bounds,
                "contiguous write is not sector aligned",
                None,
            ));
        }
        let count = sectors.len() / 512;
        let deadline = Instant::now() + Duration::from_millis(IO_MS);
        let end = u64::from(start_lba)
            .checked_add(count as u64)
            .ok_or_else(|| {
                TransportError::new(TransportErrorKind::Bounds, "sector range overflow", None)
            })?;
        if end > self.capacity {
            return Err(TransportError::new(
                TransportErrorKind::Bounds,
                "sector range outside SD capacity",
                None,
            ));
        }
        for (index, chunk) in sectors.chunks_exact(512).enumerate() {
            let remaining = deadline
                .saturating_duration_since(Instant::now())
                .as_millis();
            if remaining == 0 {
                return Err(timeout());
            }
            self.workspace.copy_from_slice(chunk);
            let card = self
                .card
                .as_mut()
                .ok_or_else(|| unavailable("SD card not initialized"))?;
            Self::run_timed(
                remaining,
                card.write(
                    start_lba + index as u32,
                    core::slice::from_ref(&*self.workspace),
                ),
            )
            .await?;
        }
        Ok(())
    }

    async fn recover_after_timeout(&mut self) {
        let _ = self.recover().await;
    }
}

fn map_error(error: sdio::MmcError) -> TransportError {
    let (kind, detail) = match error {
        sdio::MmcError::Timeout => (TransportErrorKind::Timeout, "sdio timeout"),
        sdio::MmcError::Crc => (TransportErrorKind::Integrity, "sdio CRC error"),
        sdio::MmcError::Busy => (TransportErrorKind::Timeout, "sdio card busy"),
        sdio::MmcError::Io => (TransportErrorKind::Io, "sdio hardware I/O error"),
        sdio::MmcError::Card(_) => (TransportErrorKind::Protocol, "sdio card status error"),
        sdio::MmcError::Sdio(_) => (TransportErrorKind::Protocol, "sdio function error"),
        sdio::MmcError::BlockSize => (TransportErrorKind::Protocol, "sdio block size error"),
        sdio::MmcError::BusWidth => (TransportErrorKind::Protocol, "sdio bus width error"),
        sdio::MmcError::Voltage => (TransportErrorKind::Protocol, "sdio voltage error"),
        sdio::MmcError::Unsupported => (TransportErrorKind::Protocol, "sdio unsupported operation"),
        sdio::MmcError::Signaling => (TransportErrorKind::Protocol, "sdio signaling error"),
        sdio::MmcError::CardType => (TransportErrorKind::Protocol, "sdio card type error"),
        _ => (TransportErrorKind::Protocol, "sdio protocol error"),
    };
    TransportError::new(kind, detail, None)
}
fn timeout() -> TransportError {
    TransportError::new(
        TransportErrorKind::Timeout,
        "sdio operation timed out",
        None,
    )
}
fn unavailable(detail: &'static str) -> TransportError {
    TransportError::new(TransportErrorKind::Unavailable, detail, None)
}
fn halt() -> ! {
    loop {
        esp_hal::delay::Delay::new().delay_millis(1_000);
    }
}
