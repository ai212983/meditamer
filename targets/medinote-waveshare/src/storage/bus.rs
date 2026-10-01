//! A borrowed SDHOST bus for the single-owner native SD transport.

use core::cell::RefCell;
use esp_hal::{sdmmc::Slot, Async};
use sdio::{
    BlockReadCommand, BlockWriteCommand, BusWidth, ByteReadCommand, ByteWriteCommand,
    ControlCommand, MmcBus, MmcError, TuningOp,
};

/// Delegates every bus operation through the one statically owned slot.
///
/// The borrow is held across each HAL future.  The transport is the sole owner
/// of this cell; this wrapper only makes that ownership compatible with sdio's
/// reusable `BlockDevice`.
pub struct BorrowedBus {
    pub(crate) slot: &'static RefCell<Slot<'static, 1, Async>>,
}

impl BorrowedBus {
    pub const fn new(slot: &'static RefCell<Slot<'static, 1, Async>>) -> Self {
        Self { slot }
    }
}

// This cell is private to the sole storage task. The card API has no bus
// extraction method; retaining the borrow through the HAL future both enforces
// exclusive access and releases it when that future is cancelled. No other
// task holds a handle or can contend for the borrow.
#[allow(clippy::await_holding_refcell_ref)]
impl MmcBus for BorrowedBus {
    async fn send_command<'a, C>(&mut self, cmd: C) -> Result<C::Resp<'a>, MmcError>
    where
        C: ControlCommand + 'a,
    {
        self.slot.borrow_mut().send_command(cmd).await
    }

    async fn read_blocks<'a, C>(&mut self, cmd: C, auto_stop: bool) -> Result<C::Resp<'a>, MmcError>
    where
        C: BlockReadCommand + 'a,
    {
        self.slot.borrow_mut().read_blocks(cmd, auto_stop).await
    }

    async fn write_blocks<'a, C>(
        &mut self,
        cmd: C,
        auto_stop: bool,
    ) -> Result<C::Resp<'a>, MmcError>
    where
        C: BlockWriteCommand + 'a,
    {
        self.slot.borrow_mut().write_blocks(cmd, auto_stop).await
    }

    async fn read_bytes<'a, C>(&mut self, cmd: C) -> Result<C::Resp<'a>, MmcError>
    where
        C: ByteReadCommand + 'a,
    {
        self.slot.borrow_mut().read_bytes(cmd).await
    }

    async fn write_bytes<'a, C>(&mut self, cmd: C) -> Result<C::Resp<'a>, MmcError>
    where
        C: ByteWriteCommand + 'a,
    {
        self.slot.borrow_mut().write_bytes(cmd).await
    }

    async fn init_idle(&mut self, hz: u32) -> Result<(), MmcError> {
        self.slot.borrow_mut().init_idle(hz).await
    }

    async fn tune_bus<O>(&mut self, width: BusWidth, hz: u32, op: O) -> Result<(), MmcError>
    where
        O: TuningOp,
    {
        self.slot.borrow_mut().tune_bus(width, hz, op).await
    }

    async fn wait_for_event(&mut self) -> Result<(), MmcError> {
        self.slot.borrow_mut().wait_for_event().await
    }

    fn set_bus(&mut self, width: BusWidth, hz: u32) -> Result<(), MmcError> {
        self.slot.borrow_mut().set_bus(width, hz)
    }

    fn supports_mmc(&self) -> bool {
        self.slot.borrow().supports_mmc()
    }
    fn supports_auto_stop(&self) -> bool {
        self.slot.borrow().supports_auto_stop()
    }
    fn supports_bus_width(&self) -> BusWidth {
        self.slot.borrow().supports_bus_width()
    }
    fn supports_1v8(&self) -> bool {
        self.slot.borrow().supports_1v8()
    }
    fn supports_frequency(&self) -> u32 {
        self.slot.borrow().supports_frequency()
    }
}
