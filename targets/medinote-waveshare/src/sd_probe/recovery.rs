//! Post-operation cleanup for the sole-owner native SD transport.
//!
//! Call only after the HAL future has completed or been dropped and its slot
//! borrow has returned. No other slot, task, or core may use SDHOST. This is
//! deliberately target-local: esp-hal 1.2.0 aborts DMA on cancellation but
//! leaves CARDCLRINTEN set when a write's busy wait is cancelled.

use esp_hal::{
    sdmmc::{BusWidth, Slot},
    time::{Duration, Instant},
    Async,
};

const DMA_ENABLE: u32 = 1 << 5;
const USE_INTERNAL_DMA: u32 = 1 << 25;
const RESET_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub enum RecoveryError {
    ResetTimeout,
    DmaStillEnabled,
    BusConfiguration,
}

fn registers() -> &'static esp32s3::sdhost::RegisterBlock {
    // SAFETY: this private target module is called only between operations by
    // the sole SDHOST owner. HAL's future DropGuard has released its engine
    // session; no outstanding future or second core can touch these registers.
    // No peripheral handle is stolen and no DMA buffer address is installed.
    unsafe { &*esp32s3::SDHOST::ptr() }
}

fn wait_reset(r: &esp32s3::sdhost::RegisterBlock) -> Result<(), RecoveryError> {
    let start = Instant::now();
    loop {
        let ctrl = r.ctrl().read();
        if !ctrl.controller_reset().bit_is_set()
            && !ctrl.fifo_reset().bit_is_set()
            && !ctrl.dma_reset().bit_is_set()
            && !r.bmod().read().swr().bit_is_set()
        {
            return Ok(());
        }
        if start.elapsed() >= RESET_TIMEOUT {
            return Err(RecoveryError::ResetTimeout);
        }
        core::hint::spin_loop();
    }
}

/// Establish stopped DMA before any transfer buffer is read or reused.
///
/// On error the caller must halt with the buffer retained, without accessing
/// it or attempting another operation: quiescence has not been established.
pub fn quiesce() -> Result<(), RecoveryError> {
    let r = registers();
    r.intmask().write(|w| unsafe { w.bits(0) });
    r.idinten().write(|w| unsafe { w.bits(0) });
    r.cardthrctl().modify(|_, w| w.cardclrinten().clear_bit());
    r.ctrl()
        .modify(|rd, w| unsafe { w.bits(rd.bits() & !(DMA_ENABLE | USE_INTERNAL_DMA)) });
    r.bmod().modify(|_, w| {
        w.de().clear_bit();
        w.fb().clear_bit();
        w.swr().set_bit()
    });
    r.ctrl().modify(|_, w| {
        w.fifo_reset().set_bit();
        w.dma_reset().set_bit()
    });
    wait_reset(r)?;
    if r.ctrl().read().bits() & (DMA_ENABLE | USE_INTERNAL_DMA) != 0
        || r.bmod().read().de().bit_is_set()
    {
        return Err(RecoveryError::DmaStillEnabled);
    }
    r.rintsts().write(|w| unsafe { w.bits(u32::MAX) });
    r.idsts().write(|w| unsafe { w.bits(u32::MAX) });
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// Reset the engine and invalidate HAL's cached bus configuration before a
/// fresh borrowed-slot block device reacquires the card.
pub fn reset(slot: &mut Slot<'_, 1, Async>) -> Result<(), RecoveryError> {
    quiesce()?;
    let r = registers();
    // Preserve the module clock and GPIO routing. A peripheral/system reset
    // would invalidate state that the existing HAL slot cannot reconstruct.
    r.ctrl().modify(|_, w| {
        w.controller_reset().set_bit();
        w.fifo_reset().set_bit();
        w.dma_reset().set_bit()
    });
    wait_reset(r)?;
    r.tmout().write(|w| unsafe {
        w.response_timeout().bits(0xFF);
        w.data_timeout().bits(0xFF_FFFF)
    });
    r.cardthrctl().modify(|_, w| w.cardclrinten().clear_bit());
    r.rintsts().write(|w| unsafe { w.bits(u32::MAX) });
    r.idsts().write(|w| unsafe { w.bits(u32::MAX) });
    r.ctrl().modify(|_, w| w.int_enable().set_bit());
    // init_idle acquires the engine before updating this cache itself. Mark
    // it dirty first so that acquisition restores the reset hardware at 1 bit
    // and 400 kHz rather than trusting the previous active-slot cache.
    slot.set_bus_low_level(BusWidth::Bit1, 400_000)
        .map_err(|_| RecoveryError::BusConfiguration)
}

/// Gate completion delivery for the sole-owner probe. Injection may re-enable
/// it while a HAL operation is pending, after the receiver input is prepared.
/// Only this control bit is changed; the ISR owns its status/interrupt masks.
#[cfg(feature = "sd-transport-probe")]
pub fn set_interrupt_delivery(enabled: bool) {
    esp_hal::peripherals::SDHOST::regs()
        .ctrl()
        .modify(|_, w| w.int_enable().bit(enabled));
}
