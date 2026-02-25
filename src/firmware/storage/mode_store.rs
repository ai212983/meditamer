use embedded_storage::{ReadStorage, Storage};
use esp_storage::FlashStorage;

use super::super::{
    config::{MODE_STORE_MAGIC, MODE_STORE_RECORD_LEN, MODE_STORE_VERSION},
    types::{DisplayMode, RuntimeMode, RuntimeServices},
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
        self.load_modes().map(|(display_mode, _)| display_mode)
    }

    pub(crate) fn load_runtime_services(&mut self) -> Option<RuntimeServices> {
        self.load_modes().map(|(_, services)| services)
    }

    pub(crate) fn save_mode(&mut self, mode: DisplayMode) {
        let runtime_services = self
            .load_runtime_services()
            .unwrap_or(RuntimeServices::normal());
        if self.load_mode() == Some(mode) {
            return;
        }
        self.save_modes(mode, runtime_services);
    }

    pub(crate) fn save_runtime_services(&mut self, services: RuntimeServices) {
        let display_mode = self.load_mode().unwrap_or(DisplayMode::Shanshui);
        if self.load_runtime_services() == Some(services) {
            return;
        }
        self.save_modes(display_mode, services);
    }

    fn save_modes(&mut self, display_mode: DisplayMode, runtime_services: RuntimeServices) {
        let mut record = [0xFFu8; MODE_STORE_RECORD_LEN];
        record[0..4].copy_from_slice(&MODE_STORE_MAGIC.to_le_bytes());
        record[4] = MODE_STORE_VERSION;
        record[5] = display_mode.as_persisted();
        record[6] = runtime_services.as_persisted();
        record[MODE_STORE_RECORD_LEN - 1] = checksum8(&record[..MODE_STORE_RECORD_LEN - 1]);
        let _ = self.flash.write(self.offset, &record);
    }

    fn load_modes(&mut self) -> Option<(DisplayMode, RuntimeServices)> {
        let record = self.load_record()?;
        match record[4] {
            1 => {
                let display_mode = DisplayMode::from_persisted(record[5])?;
                Some((display_mode, RuntimeServices::normal()))
            }
            2 => {
                let display_mode = DisplayMode::from_persisted(record[5])?;
                let runtime_mode = RuntimeMode::from_persisted(record[6])?;
                Some((display_mode, runtime_mode.as_services()))
            }
            MODE_STORE_VERSION => {
                let display_mode = DisplayMode::from_persisted(record[5])?;
                let runtime_services = RuntimeServices::from_persisted(record[6]);
                Some((display_mode, runtime_services))
            }
            _ => None,
        }
    }

    fn load_record(&mut self) -> Option<[u8; MODE_STORE_RECORD_LEN]> {
        let mut record = [0u8; MODE_STORE_RECORD_LEN];
        self.flash.read(self.offset, &mut record).ok()?;
        if record.iter().all(|&byte| byte == 0xFF) {
            return None;
        }
        if u32::from_le_bytes([record[0], record[1], record[2], record[3]]) != MODE_STORE_MAGIC {
            return None;
        }
        let expected = checksum8(&record[..MODE_STORE_RECORD_LEN - 1]);
        if record[MODE_STORE_RECORD_LEN - 1] != expected {
            return None;
        }
        Some(record)
    }
}

fn checksum8(bytes: &[u8]) -> u8 {
    let mut acc = 0x5Au8;
    for &byte in bytes {
        acc ^= byte.rotate_left(1);
    }
    acc
}
