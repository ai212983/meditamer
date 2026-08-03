use embedded_storage::{ReadStorage, Storage};
use esp_storage::FlashStorage;

use crate::firmware::config::{
    APP_STATE_STORE_MAGIC, APP_STATE_STORE_RECORD_LEN, APP_STATE_STORE_VERSION,
};

use super::snapshot::AppStateSnapshot;
use super::types::{DiagKind, DiagTargets, ServiceFlags};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct PersistedAppState {
    pub(crate) services: ServiceFlags,
    pub(crate) diag_kind: DiagKind,
    pub(crate) diag_targets: DiagTargets,
}

impl Default for PersistedAppState {
    fn default() -> Self {
        Self {
            services: ServiceFlags::normal(),
            diag_kind: DiagKind::None,
            diag_targets: DiagTargets::none(),
        }
    }
}

impl PersistedAppState {
    pub(crate) fn from_snapshot(snapshot: AppStateSnapshot) -> Self {
        Self {
            services: snapshot.services,
            diag_kind: snapshot.diag_kind,
            diag_targets: snapshot.diag_targets,
        }
    }

    pub(crate) fn update_from_snapshot(&mut self, snapshot: AppStateSnapshot) {
        self.services = snapshot.services;
        self.diag_kind = snapshot.diag_kind;
        self.diag_targets = snapshot.diag_targets;
    }

    pub(crate) fn record_bytes(self) -> [u8; APP_STATE_STORE_RECORD_LEN] {
        let mut record = [0xFFu8; APP_STATE_STORE_RECORD_LEN];
        record[0..4].copy_from_slice(&APP_STATE_STORE_MAGIC.to_le_bytes());
        record[4] = APP_STATE_STORE_VERSION;
        record[5] = self.services.as_bits();
        record[6] = self.diag_kind.as_u8();
        record[7] = self.diag_targets.as_persisted();
        record[APP_STATE_STORE_RECORD_LEN - 1] =
            checksum8(&record[..APP_STATE_STORE_RECORD_LEN - 1]);
        record
    }

    pub(crate) fn from_record(record: &[u8; APP_STATE_STORE_RECORD_LEN]) -> Option<Self> {
        if record.iter().all(|&byte| byte == 0xFF) {
            return None;
        }
        if u32::from_le_bytes([record[0], record[1], record[2], record[3]]) != APP_STATE_STORE_MAGIC
        {
            return None;
        }
        if record[4] != APP_STATE_STORE_VERSION {
            return None;
        }
        let expected = checksum8(&record[..APP_STATE_STORE_RECORD_LEN - 1]);
        if expected != record[APP_STATE_STORE_RECORD_LEN - 1] {
            return None;
        }
        Some(Self {
            services: ServiceFlags::from_bits(record[5]),
            diag_kind: DiagKind::from_u8(record[6])?,
            diag_targets: DiagTargets::from_persisted(record[7]),
        })
    }
}

pub(crate) struct AppStateStore<'d> {
    flash: FlashStorage<'d>,
    offset: u32,
}

impl<'d> AppStateStore<'d> {
    pub(crate) fn new(flash_peripheral: esp_hal::peripherals::FLASH<'d>) -> Self {
        let flash = FlashStorage::new(flash_peripheral).multicore_auto_park();
        let capacity = flash.capacity() as u32;
        let offset = capacity.saturating_sub(FlashStorage::SECTOR_SIZE);
        Self { flash, offset }
    }

    pub(crate) fn load_state(&mut self) -> Option<PersistedAppState> {
        let mut record = [0u8; APP_STATE_STORE_RECORD_LEN];
        self.flash.read(self.offset, &mut record).ok()?;
        PersistedAppState::from_record(&record)
    }

    pub(crate) fn save_state(&mut self, persisted: PersistedAppState) {
        let current = self.load_state();
        if current == Some(persisted) {
            return;
        }
        let record = persisted.record_bytes();
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

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    #[test]
    fn persisted_roundtrip_default() {
        let state = PersistedAppState::default();
        let record = state.record_bytes();
        let decoded = PersistedAppState::from_record(&record).expect("decode");
        assert_eq!(decoded, state);
    }

    #[test]
    fn rejects_legacy_version() {
        let mut record = PersistedAppState::default().record_bytes();
        record[4] = 2;
        record[APP_STATE_STORE_RECORD_LEN - 1] =
            checksum8(&record[..APP_STATE_STORE_RECORD_LEN - 1]);
        assert!(PersistedAppState::from_record(&record).is_none());
    }
}
