//! Medinote/Waveshare binding for the shared internal-flash credential store.

use core::cell::RefCell;

use critical_section::Mutex;
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};
use esp_storage::FlashStorage;
use netstack::config::{
    flash_store::{self, CredentialFlash, FlashStoreError},
    WifiConfigResultCode, WifiCredentials,
};

const WIFI_CONFIG_PARTITION_OFFSET: u32 = 0x210000;

static FLASH: Mutex<RefCell<Option<FlashStorage<'static>>>> = Mutex::new(RefCell::new(None));

pub fn initialize(peripheral: esp_hal::peripherals::FLASH<'static>) {
    critical_section::with(|cs| {
        let mut slot = FLASH.borrow_ref_mut(cs);
        assert!(slot.is_none(), "flash owner initialized twice");
        *slot = Some(FlashStorage::new(peripheral).multicore_auto_park());
    });
}

fn with_flash<R>(operation: impl FnOnce(&mut FlashStorage<'static>) -> R) -> R {
    struct Lease(Option<FlashStorage<'static>>);

    impl Drop for Lease {
        fn drop(&mut self) {
            if let Some(flash) = self.0.take() {
                critical_section::with(|cs| {
                    let mut slot = FLASH.borrow_ref_mut(cs);
                    assert!(slot.is_none(), "flash owner replaced while leased");
                    *slot = Some(flash);
                });
            }
        }
    }

    // The flash driver handles the interrupt/cache restrictions of each
    // physical operation. Hold our critical section only to transfer its
    // exclusive owner; a sector erase and verification must not extend it.
    let flash = critical_section::with(|cs| {
        FLASH
            .borrow_ref_mut(cs)
            .take()
            .expect("flash owner absent or already leased")
    });
    let mut lease = Lease(Some(flash));
    operation(lease.0.as_mut().expect("flash lease is held"))
}

struct Partition<'a> {
    flash: &'a mut FlashStorage<'static>,
}

impl Partition<'_> {
    fn absolute(&self, offset: u32, len: u32) -> Option<(u32, u32)> {
        let end = offset.checked_add(len)?;
        if end > flash_store::PARTITION_SIZE {
            return None;
        }
        let from = WIFI_CONFIG_PARTITION_OFFSET.checked_add(offset)?;
        Some((from, from.checked_add(len)?))
    }
}

impl CredentialFlash for Partition<'_> {
    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> bool {
        let Some((from, _)) = self.absolute(offset, bytes.len() as u32) else {
            return false;
        };
        ReadNorFlash::read(self.flash, from, bytes).is_ok()
    }

    fn erase(&mut self, offset: u32, len: u32) -> bool {
        let Some((from, to)) = self.absolute(offset, len) else {
            return false;
        };
        NorFlash::erase(self.flash, from, to).is_ok()
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> bool {
        let Some((from, _)) = self.absolute(offset, bytes.len() as u32) else {
            return false;
        };
        NorFlash::write(self.flash, from, bytes).is_ok()
    }
}

pub fn load() -> Result<Option<WifiCredentials>, FlashStoreError> {
    with_flash(|flash| flash_store::load(&mut Partition { flash }))
}

pub fn store(credentials: &WifiCredentials) -> WifiConfigResultCode {
    let result = with_flash(|flash| flash_store::store(&mut Partition { flash }, credentials));
    match result {
        Ok(()) => WifiConfigResultCode::Ok,
        Err(FlashStoreError::InvalidCredentials) => WifiConfigResultCode::InvalidData,
        Err(error) => {
            console::println!("WIFI_CREDENTIAL_STORE status=error phase={error:?}");
            WifiConfigResultCode::OperationFailed
        }
    }
}
