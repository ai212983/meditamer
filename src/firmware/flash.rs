use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};

use critical_section::Mutex;
use embedded_storage::{
    nor_flash::{NorFlash, ReadNorFlash},
    ReadStorage, Storage,
};
use esp_hal::{
    peripherals::CPU_CTRL,
    system::{Cpu, CpuControl},
};
use esp_storage::{FlashStorage, FlashStorageError};

static FLASH: Mutex<RefCell<Option<FlashStorage<'static>>>> = Mutex::new(RefCell::new(None));
static UPDATE_OTHER_CORE_PARKED: AtomicBool = AtomicBool::new(false);

pub(crate) fn initialize(peripheral: esp_hal::peripherals::FLASH<'static>) {
    critical_section::with(|cs| {
        let mut slot = FLASH.borrow_ref_mut(cs);
        assert!(slot.is_none(), "flash owner initialized twice");
        *slot = Some(FlashStorage::new(peripheral).multicore_auto_park());
    });
}

pub(crate) fn with<R>(operation: impl FnOnce(&mut FlashStorage<'static>) -> R) -> R {
    critical_section::with(|cs| {
        let mut slot = FLASH.borrow_ref_mut(cs);
        operation(slot.as_mut().expect("flash owner not initialized"))
    })
}

pub(crate) fn read(offset: u32, bytes: &mut [u8]) -> Result<(), FlashStorageError> {
    with(|flash| ReadStorage::read(flash, offset, bytes))
}

pub(crate) fn replace(offset: u32, bytes: &[u8]) -> Result<(), FlashStorageError> {
    with(|flash| Storage::write(flash, offset, bytes))
}

// Only src/updater/install.rs (factory-updater, minus its sd-qual-push bench variant, which
// stages onto SD instead of writing raw flash) erases/writes/reads flash directly now that
// ADR-0014 Phase 5 removed the default binary's own signed-image write path; other feature
// combinations legitimately never call these.
#[cfg_attr(
    not(all(feature = "factory-updater", not(feature = "sd-qual-push"))),
    allow(dead_code)
)]
pub(crate) fn erase(from: u32, to: u32) -> Result<(), FlashStorageError> {
    with(|flash| NorFlash::erase(flash, from, to))
}

#[cfg_attr(
    not(all(feature = "factory-updater", not(feature = "sd-qual-push"))),
    allow(dead_code)
)]
pub(crate) fn write(offset: u32, bytes: &[u8]) -> Result<(), FlashStorageError> {
    with(|flash| NorFlash::write(flash, offset, bytes))
}

#[cfg_attr(
    not(all(feature = "factory-updater", not(feature = "sd-qual-push"))),
    allow(dead_code)
)]
pub(crate) fn read_aligned(offset: u32, bytes: &mut [u8]) -> Result<(), FlashStorageError> {
    with(|flash| ReadNorFlash::read(flash, offset, bytes))
}

pub(crate) fn unpark_other_core_after_update() -> bool {
    if !UPDATE_OTHER_CORE_PARKED.swap(false, Ordering::AcqRel) {
        return false;
    }
    let mut control = CpuControl::new(unsafe { CPU_CTRL::steal() });
    for other in Cpu::other() {
        control.unpark_core(other);
    }
    true
}
