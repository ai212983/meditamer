use embedded_storage::{ReadStorage, Storage};
use esp_storage::FlashStorage;

use super::{
    config::{MODE_STORE_MAGIC, MODE_STORE_RECORD_LEN, MODE_STORE_VERSION},
    types::DisplayMode,
};

pub(crate) struct ModeStore<'d> {
    flash: FlashStorage<'d>,
    offset: u32,
}

impl<'d> ModeStore<'d> {
    pub(crate) fn new(flash_peripheral: esp_hal::peripherals::FLASH<'d>) -> Self {
        let flash = FlashStorage::new(flash_peripheral).multicore_auto_park();
        let capacity = flash.capacity() as u32;
        let offset = capacity.saturating_sub(FlashStorage::SECTOR_SIZE);
        Self { flash, offset }
    }

    pub(crate) fn load_mode(&mut self) -> Option<DisplayMode> {
        let mut record = [0u8; MODE_STORE_RECORD_LEN];
        self.flash.read(self.offset, &mut record).ok()?;
        if record.iter().all(|&byte| byte == 0xFF) {
            return None;
        }
        if u32::from_le_bytes([record[0], record[1], record[2], record[3]]) != MODE_STORE_MAGIC {
            return None;
        }
        if record[4] != MODE_STORE_VERSION {
            return None;
        }
        let expected = checksum8(&record[..MODE_STORE_RECORD_LEN - 1]);
        if record[MODE_STORE_RECORD_LEN - 1] != expected {
            return None;
        }
        DisplayMode::from_persisted(record[5])
    }

    pub(crate) fn save_mode(&mut self, mode: DisplayMode) {
        if self.load_mode() == Some(mode) {
            return;
        }

        let mut record = [0xFFu8; MODE_STORE_RECORD_LEN];
        record[0..4].copy_from_slice(&MODE_STORE_MAGIC.to_le_bytes());
        record[4] = MODE_STORE_VERSION;
        record[5] = mode.as_persisted();
        record[MODE_STORE_RECORD_LEN - 1] = checksum8(&record[..MODE_STORE_RECORD_LEN - 1]);
        let _ = self.flash.write(self.offset, &record);
    }
}

fn checksum8(bytes: &[u8]) -> u8 {
    let mut acc = 0x5Au8;
    for &byte in bytes {
        acc ^= byte.rotate_left(1);
    }
    acc
}
