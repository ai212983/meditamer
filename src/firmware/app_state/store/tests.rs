use super::*;

#[derive(Clone, Copy)]
enum ReplaceFault {
    None,
    Partial(usize),
    CorruptAfterWrite,
    VerificationReadError,
}

struct FakeStorage {
    sectors: [[u8; APP_STATE_STORE_SECTOR_SIZE as usize]; 2],
    legacy: [u8; LEGACY_RECORD_LEN],
    replace_fault: ReplaceFault,
    fail_read_once_at: Option<u32>,
}

impl FakeStorage {
    fn new() -> Self {
        Self {
            sectors: [[0xFF; APP_STATE_STORE_SECTOR_SIZE as usize]; 2],
            legacy: [0xFF; LEGACY_RECORD_LEN],
            replace_fault: ReplaceFault::None,
            fail_read_once_at: None,
        }
    }

    fn fail_next_replace(&mut self, fault: ReplaceFault) {
        self.replace_fault = fault;
    }

    fn install_previous_record(&mut self, index: usize, state: PersistedAppState, generation: u32) {
        let mut record = [0xFF; PREVIOUS_RECORD_LEN];
        record[0..4].copy_from_slice(&APP_STATE_STORE_MAGIC.to_le_bytes());
        record[4] = PREVIOUS_VERSION;
        record[5] = state.services.as_bits();
        record[6] = state.diag_kind.as_u8();
        record[7] = state.diag_targets.as_persisted();
        record[8..12].copy_from_slice(&generation.to_le_bytes());
        let checksum = crc32(&record[..PREVIOUS_CRC_OFFSET]);
        record[PREVIOUS_CRC_OFFSET..].copy_from_slice(&checksum.to_le_bytes());
        self.sectors[index][..PREVIOUS_RECORD_LEN].copy_from_slice(&record);
    }

    fn sector_range(offset: u32, len: usize) -> Option<(usize, usize)> {
        let relative = offset.checked_sub(APP_STATE_STORE_OFFSET)?;
        let index = usize::try_from(relative / APP_STATE_STORE_SECTOR_SIZE).ok()?;
        if index >= 2 {
            return None;
        }
        let within = usize::try_from(relative % APP_STATE_STORE_SECTOR_SIZE).ok()?;
        (within.checked_add(len)? <= APP_STATE_STORE_SECTOR_SIZE as usize)
            .then_some((index, within))
    }
}

impl StoreStorage for FakeStorage {
    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> bool {
        if self.fail_read_once_at == Some(offset) {
            self.fail_read_once_at = None;
            return false;
        }
        if offset == APP_STATE_LEGACY_OFFSET && bytes.len() == LEGACY_RECORD_LEN {
            bytes.copy_from_slice(&self.legacy);
            return true;
        }
        let Some((index, within)) = Self::sector_range(offset, bytes.len()) else {
            return false;
        };
        bytes.copy_from_slice(&self.sectors[index][within..within + bytes.len()]);
        true
    }

    fn replace(&mut self, offset: u32, bytes: &[u8]) -> bool {
        let Some((index, within)) = Self::sector_range(offset, bytes.len()) else {
            return false;
        };
        self.sectors[index].fill(0xFF);
        let fault = core::mem::replace(&mut self.replace_fault, ReplaceFault::None);
        match fault {
            ReplaceFault::None => {
                self.sectors[index][within..within + bytes.len()].copy_from_slice(bytes);
                true
            }
            ReplaceFault::Partial(count) => {
                let count = count.min(bytes.len());
                self.sectors[index][within..within + count].copy_from_slice(&bytes[..count]);
                false
            }
            ReplaceFault::CorruptAfterWrite => {
                self.sectors[index][within..within + bytes.len()].copy_from_slice(bytes);
                self.sectors[index][within + SETTINGS_AMBIENT_OFFSET] ^= 1;
                true
            }
            ReplaceFault::VerificationReadError => {
                self.sectors[index][within..within + bytes.len()].copy_from_slice(bytes);
                self.fail_read_once_at = Some(offset);
                true
            }
        }
    }
}

fn state_with_upload() -> PersistedAppState {
    PersistedAppState {
        services: ServiceFlags {
            upload_enabled: true,
        },
        diag_kind: DiagKind::Debug,
        diag_targets: DiagTargets::from_persisted(0b1_0011),
    }
}

fn settings_with_ambient(local: u16) -> PersistedUiSettings {
    PersistedUiSettings {
        ambient_binding: Some(EntryId::new(1, local)),
        ..PersistedUiSettings::default()
    }
}

#[test]
fn persisted_roundtrip_default() {
    let state = PersistedAppState::default();
    let record = state.record_bytes();
    assert_eq!(PersistedAppState::from_record(&record), Some(state));
}

#[test]
fn non_default_ui_settings_roundtrip() {
    let mut settings = PersistedUiSettings {
        ambient_binding: Some(EntryId::new(1, 1)),
        startup_entry: Some(EntryId::new(1, 2)),
        enablement_configured: true,
        startup_overlays_configured: true,
        ..PersistedUiSettings::default()
    };
    settings.pins.push(EntryId::new(1, 2)).unwrap();
    settings.enabled_overlays.push(EntryId::new(1, 5)).unwrap();
    settings.startup_overlays.push(EntryId::new(1, 5)).unwrap();
    let stored = StoredRecord {
        state: PersistedAppState::default(),
        ui_settings: settings,
        generation: 7,
    };
    assert_eq!(StoredRecord::from_bytes(&stored.to_bytes()), Some(stored));
}

#[test]
fn rejects_wrong_version_crc_and_lengths() {
    let mut record = PersistedAppState::default().record_bytes();
    record[4] = 3;
    assert!(PersistedAppState::from_record(&record).is_none());
    let mut record = PersistedAppState::default().record_bytes();
    record[5] ^= 1;
    assert!(PersistedAppState::from_record(&record).is_none());
    let mut record = PersistedAppState::default().record_bytes();
    record[SETTINGS_PIN_COUNT_OFFSET] = (UI_SETTINGS_CAPACITY + 1) as u8;
    let checksum = crc32(&record[..CRC_OFFSET]);
    record[CRC_OFFSET..].copy_from_slice(&checksum.to_le_bytes());
    assert!(PersistedAppState::from_record(&record).is_none());

    let mut record = PersistedAppState::default().record_bytes();
    record[SETTINGS_FLAGS_OFFSET] |= 1 << 7;
    let checksum = crc32(&record[..CRC_OFFSET]);
    record[CRC_OFFSET..].copy_from_slice(&checksum.to_le_bytes());
    assert!(PersistedAppState::from_record(&record).is_none());

    let mut record = PersistedAppState::default().record_bytes();
    record[120] = 0;
    let checksum = crc32(&record[..CRC_OFFSET]);
    record[CRC_OFFSET..].copy_from_slice(&checksum.to_le_bytes());
    assert!(PersistedAppState::from_record(&record).is_none());
}

#[test]
fn wrapping_generation_order_is_stable() {
    assert!(generation_is_newer(0, u32::MAX));
    assert!(!generation_is_newer(u32::MAX, 0));
}

#[test]
fn decodes_preserved_version_2_device_record() {
    let record = [
        0x53, 0x50, 0x50, 0x41, 0x02, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00, 0x01, 0x2f, 0x00, 0x32,
        0x02, 0x47, 0x00, 0x27, 0x02, 0x34, 0x00, 0x23, 0x02, 0x34, 0x00, 0x23, 0x02, 0xff, 0xff,
        0xff, 0x7b,
    ];
    let (version, decoded) = decode_legacy_record(&record).expect("preserved record must migrate");
    assert_eq!(version, 2);
    assert_eq!(
        legacy_state_for_migration(version, decoded),
        PersistedAppState::default()
    );
}

#[test]
fn interrupted_target_replacement_preserves_previous_generation() {
    for written_bytes in [0, 37] {
        let mut storage = FakeStorage::new();
        let original = StoredRecord {
            state: state_with_upload(),
            ui_settings: settings_with_ambient(1),
            generation: 7,
        };
        assert!(write_record(&mut storage, 0, &original));

        storage.fail_next_replace(ReplaceFault::Partial(written_bytes));
        assert!(!save_ui_settings_to(&mut storage, settings_with_ambient(2)));

        assert_eq!(load_record_with_migration(&mut storage), Some(original));
    }
}

#[test]
fn corrupt_newest_record_falls_back_to_previous_generation() {
    let mut storage = FakeStorage::new();
    let previous = StoredRecord {
        state: state_with_upload(),
        ui_settings: settings_with_ambient(1),
        generation: 10,
    };
    let newest = StoredRecord {
        state: PersistedAppState::default(),
        ui_settings: settings_with_ambient(2),
        generation: 11,
    };
    assert!(write_record(&mut storage, 0, &previous));
    assert!(write_record(&mut storage, 1, &newest));
    storage.sectors[1][SETTINGS_AMBIENT_OFFSET] ^= 1;

    assert_eq!(load_record_with_migration(&mut storage), Some(previous));
}

#[test]
fn write_verification_failures_leave_a_recoverable_generation() {
    for fault in [
        ReplaceFault::CorruptAfterWrite,
        ReplaceFault::VerificationReadError,
    ] {
        let mut storage = FakeStorage::new();
        let previous = StoredRecord {
            state: state_with_upload(),
            ui_settings: settings_with_ambient(1),
            generation: 3,
        };
        assert!(write_record(&mut storage, 0, &previous));

        storage.fail_next_replace(fault);
        assert!(!save_ui_settings_to(&mut storage, settings_with_ambient(2)));
        assert_eq!(read_record(&mut storage, 0), Some(previous.clone()));

        let recovered = load_record_with_migration(&mut storage).unwrap();
        assert_eq!(recovered.state, previous.state);
        assert!(recovered == previous || recovered.ui_settings == settings_with_ambient(2));
    }
}

#[test]
fn version_four_migration_uses_opposite_sector_and_retries_after_failure() {
    let mut storage = FakeStorage::new();
    let previous_state = state_with_upload();
    storage.install_previous_record(1, previous_state, 9);
    storage.fail_next_replace(ReplaceFault::Partial(0));

    let first_load = load_record_with_migration(&mut storage).unwrap();
    assert_eq!(first_load.state, previous_state);
    assert_eq!(first_load.generation, 10);
    assert!(current_record(&mut storage).is_none());
    assert_eq!(current_previous_record(&mut storage).unwrap().0, 1);

    let second_load = load_record_with_migration(&mut storage).unwrap();
    assert_eq!(second_load, first_load);
    let (index, migrated) = current_record(&mut storage).unwrap();
    assert_eq!(index, 0);
    assert_eq!(migrated, first_load);
}

#[test]
fn lifecycle_and_ui_writes_copy_forward_the_other_owner() {
    let mut storage = FakeStorage::new();
    let lifecycle = state_with_upload();
    let settings = settings_with_ambient(7);

    assert!(save_state_to(&mut storage, lifecycle));
    assert!(save_ui_settings_to(&mut storage, settings.clone()));
    let after_ui = load_record_with_migration(&mut storage).unwrap();
    assert_eq!(after_ui.state, lifecycle);
    assert_eq!(after_ui.ui_settings, settings);

    assert!(save_state_to(&mut storage, PersistedAppState::default()));
    let after_lifecycle = load_record_with_migration(&mut storage).unwrap();
    assert_eq!(after_lifecycle.state, PersistedAppState::default());
    assert_eq!(after_lifecycle.ui_settings, settings);
}
