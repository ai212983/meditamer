use heapless::Vec;

use crate::firmware::{
    config::{
        APP_STATE_LEGACY_OFFSET, APP_STATE_STORE_MAGIC, APP_STATE_STORE_OFFSET,
        APP_STATE_STORE_RECORD_LEN, APP_STATE_STORE_SECTOR_SIZE, APP_STATE_STORE_VERSION,
    },
    flash,
    ui::shell::{
        catalogue::EntryId,
        settings::{PersistedUiSettings, UI_SETTINGS_CAPACITY},
    },
};

use super::snapshot::AppStateSnapshot;
use super::types::{DiagKind, DiagTargets, ServiceFlags};

trait StoreStorage {
    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> bool;
    fn replace(&mut self, offset: u32, bytes: &[u8]) -> bool;
}

struct DeviceStorage;

impl StoreStorage for DeviceStorage {
    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> bool {
        flash::read(offset, bytes).is_ok()
    }

    fn replace(&mut self, offset: u32, bytes: &[u8]) -> bool {
        flash::replace(offset, bytes).is_ok()
    }
}

#[cfg(target_os = "none")]
macro_rules! store_log {
    ($($argument:tt)*) => {
        esp_println::println!($($argument)*)
    };
}

#[cfg(not(target_os = "none"))]
macro_rules! store_log {
    ($($argument:tt)*) => {
        let _ = core::format_args!($($argument)*);
    };
}

const LEGACY_VERSION_MIN: u8 = 2;
const LEGACY_VERSION_MAX: u8 = 3;
const LEGACY_RECORD_LEN: usize = 32;
const PREVIOUS_VERSION: u8 = 4;
const PREVIOUS_RECORD_LEN: usize = 64;
const PREVIOUS_CRC_OFFSET: usize = PREVIOUS_RECORD_LEN - 4;
const CRC_OFFSET: usize = APP_STATE_STORE_RECORD_LEN - 4;

const SETTINGS_FLAGS_OFFSET: usize = 12;
const SETTINGS_PIN_COUNT_OFFSET: usize = 13;
const SETTINGS_ENABLED_COUNT_OFFSET: usize = 14;
const SETTINGS_STARTUP_OVERLAY_COUNT_OFFSET: usize = 15;
const SETTINGS_AMBIENT_OFFSET: usize = 16;
const SETTINGS_STARTUP_ENTRY_OFFSET: usize = 20;
const SETTINGS_PINS_OFFSET: usize = 24;
const SETTINGS_ENABLED_OFFSET: usize = 56;
const SETTINGS_STARTUP_OVERLAYS_OFFSET: usize = 88;
const SETTINGS_ID_LEN: usize = 4;
const SETTINGS_AMBIENT_PRESENT: u8 = 1 << 0;
const SETTINGS_STARTUP_ENTRY_PRESENT: u8 = 1 << 1;
const SETTINGS_ENABLEMENT_CONFIGURED: u8 = 1 << 2;
const SETTINGS_STARTUP_OVERLAYS_CONFIGURED: u8 = 1 << 3;
const SETTINGS_SUPPORTED_FLAGS: u8 = SETTINGS_AMBIENT_PRESENT
    | SETTINGS_STARTUP_ENTRY_PRESENT
    | SETTINGS_ENABLEMENT_CONFIGURED
    | SETTINGS_STARTUP_OVERLAYS_CONFIGURED;

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

    #[cfg(all(test, not(target_os = "none")))]
    pub(crate) fn record_bytes(self) -> [u8; APP_STATE_STORE_RECORD_LEN] {
        StoredRecord {
            state: self,
            ui_settings: PersistedUiSettings::default(),
            generation: 1,
        }
        .to_bytes()
    }

    #[cfg(all(test, not(target_os = "none")))]
    pub(crate) fn from_record(record: &[u8; APP_STATE_STORE_RECORD_LEN]) -> Option<Self> {
        StoredRecord::from_bytes(record).map(|stored| stored.state)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StoredRecord {
    state: PersistedAppState,
    ui_settings: PersistedUiSettings,
    generation: u32,
}

impl StoredRecord {
    fn to_bytes(&self) -> [u8; APP_STATE_STORE_RECORD_LEN] {
        let mut record = [0xFFu8; APP_STATE_STORE_RECORD_LEN];
        record[0..4].copy_from_slice(&APP_STATE_STORE_MAGIC.to_le_bytes());
        record[4] = APP_STATE_STORE_VERSION;
        record[5] = self.state.services.as_bits();
        record[6] = self.state.diag_kind.as_u8();
        record[7] = self.state.diag_targets.as_persisted();
        record[8..12].copy_from_slice(&self.generation.to_le_bytes());
        encode_ui_settings(&mut record, &self.ui_settings);
        let checksum = crc32(&record[..CRC_OFFSET]);
        record[CRC_OFFSET..].copy_from_slice(&checksum.to_le_bytes());
        record
    }

    fn from_bytes(record: &[u8; APP_STATE_STORE_RECORD_LEN]) -> Option<Self> {
        if record.iter().all(|&byte| byte == 0xFF)
            || u32::from_le_bytes(record[0..4].try_into().ok()?) != APP_STATE_STORE_MAGIC
            || record[4] != APP_STATE_STORE_VERSION
        {
            return None;
        }
        let expected = u32::from_le_bytes(record[CRC_OFFSET..].try_into().ok()?);
        if crc32(&record[..CRC_OFFSET]) != expected {
            return None;
        }
        Some(Self {
            state: PersistedAppState {
                services: ServiceFlags::from_bits(record[5]),
                diag_kind: DiagKind::from_u8(record[6])?,
                diag_targets: DiagTargets::from_persisted(record[7]),
            },
            ui_settings: decode_ui_settings(record)?,
            generation: u32::from_le_bytes(record[8..12].try_into().ok()?),
        })
    }
}

#[derive(Clone, Copy)]
struct PreviousRecord {
    state: PersistedAppState,
    generation: u32,
}

impl PreviousRecord {
    fn into_current(self) -> StoredRecord {
        StoredRecord {
            state: self.state,
            ui_settings: PersistedUiSettings::default(),
            generation: self.generation.wrapping_add(1),
        }
    }
}

pub(crate) struct AppStateStore;

impl AppStateStore {
    pub(crate) const fn new() -> Self {
        Self
    }

    pub(crate) fn load_state(&mut self) -> Option<PersistedAppState> {
        load_record_with_migration(&mut DeviceStorage).map(|record| record.state)
    }

    pub(crate) fn load_ui_settings(&mut self) -> Option<PersistedUiSettings> {
        load_record_with_migration(&mut DeviceStorage).map(|record| record.ui_settings)
    }

    pub(crate) fn save_state(&mut self, persisted: PersistedAppState) {
        if !save_state_to(&mut DeviceStorage, persisted) {
            store_log!("APP_STATE_SAVE status=error reason=write_verify_failed");
        }
    }

    #[inline(never)]
    pub(crate) fn save_ui_settings(&mut self, settings: PersistedUiSettings) -> bool {
        let saved = save_ui_settings_to(&mut DeviceStorage, settings);
        if !saved {
            store_log!("UI_SETTINGS_SAVE status=error reason=write_verify_failed");
        }
        saved
    }
}

pub(crate) fn migration_complete() -> bool {
    current_record(&mut DeviceStorage).is_some()
}

fn default_record() -> StoredRecord {
    StoredRecord {
        state: PersistedAppState::default(),
        ui_settings: PersistedUiSettings::default(),
        generation: 0,
    }
}

fn save_state_to(storage: &mut impl StoreStorage, persisted: PersistedAppState) -> bool {
    let mut record = load_record_with_migration(storage).unwrap_or_else(default_record);
    if record.state == persisted {
        return true;
    }
    record.state = persisted;
    write_next_record(storage, record)
}

fn save_ui_settings_to(storage: &mut impl StoreStorage, settings: PersistedUiSettings) -> bool {
    let mut record = load_record_with_migration(storage).unwrap_or_else(default_record);
    if record.ui_settings == settings {
        return true;
    }
    record.ui_settings = settings;
    write_next_record(storage, record)
}

fn load_record_with_migration(storage: &mut impl StoreStorage) -> Option<StoredRecord> {
    if let Some((_, record)) = current_record(storage) {
        return Some(record);
    }

    if let Some((source, previous)) = current_previous_record(storage) {
        let migrated = previous.into_current();
        let target = 1 - source;
        let saved = write_record(storage, target, &migrated);
        log_partition_migration(saved, PREVIOUS_VERSION, target);
        return Some(migrated);
    }

    let (legacy_version, decoded_legacy) = read_legacy_record(storage)?;
    let migrated = StoredRecord {
        state: legacy_state_for_migration(legacy_version, decoded_legacy),
        ui_settings: PersistedUiSettings::default(),
        generation: 1,
    };
    let saved = write_record(storage, 0, &migrated);
    if saved {
        store_log!(
            "APP_STATE_MIGRATION status=complete source=0x{:x} source_version={} policy={} target=0x{:x}",
            APP_STATE_LEGACY_OFFSET,
            legacy_version,
            if legacy_version == 2 { "safe_defaults" } else { "preserve" },
            APP_STATE_STORE_OFFSET,
        );
    } else {
        store_log!("APP_STATE_MIGRATION status=deferred reason=write_verify_failed");
    }
    Some(migrated)
}

fn log_partition_migration(saved: bool, source_version: u8, target: usize) {
    if saved {
        store_log!(
            "APP_STATE_MIGRATION status=complete source=app_state source_version={} policy=preserve target=0x{:x}",
            source_version,
            APP_STATE_STORE_OFFSET + target as u32 * APP_STATE_STORE_SECTOR_SIZE,
        );
    } else {
        store_log!(
            "APP_STATE_MIGRATION status=deferred source_version={} reason=write_verify_failed",
            source_version,
        );
    }
}

fn write_next_record(storage: &mut impl StoreStorage, mut record: StoredRecord) -> bool {
    let current = current_record(storage);
    let (target, generation) = match current {
        Some((index, current)) => (1 - index, current.generation.wrapping_add(1)),
        None => match current_previous_record(storage) {
            Some((index, previous)) => (1 - index, previous.generation.wrapping_add(1)),
            None => (0, record.generation.max(1)),
        },
    };
    record.generation = generation;
    write_record(storage, target, &record)
}

fn current_record(storage: &mut impl StoreStorage) -> Option<(usize, StoredRecord)> {
    let first = read_record(storage, 0);
    let second = read_record(storage, 1);
    choose_newest(first, second, |record| record.generation)
}

fn current_previous_record(storage: &mut impl StoreStorage) -> Option<(usize, PreviousRecord)> {
    let first = read_previous_record(storage, 0);
    let second = read_previous_record(storage, 1);
    choose_newest(first, second, |record| record.generation)
}

fn choose_newest<T: Clone>(
    first: Option<T>,
    second: Option<T>,
    generation: impl Fn(&T) -> u32,
) -> Option<(usize, T)> {
    match (first, second) {
        (Some(a), Some(b)) => {
            if generation_is_newer(generation(&b), generation(&a)) {
                Some((1, b))
            } else {
                Some((0, a))
            }
        }
        (Some(record), None) => Some((0, record)),
        (None, Some(record)) => Some((1, record)),
        (None, None) => None,
    }
}

fn record_offset(index: usize) -> u32 {
    APP_STATE_STORE_OFFSET + index as u32 * APP_STATE_STORE_SECTOR_SIZE
}

fn read_record(storage: &mut impl StoreStorage, index: usize) -> Option<StoredRecord> {
    let mut bytes = [0u8; APP_STATE_STORE_RECORD_LEN];
    storage
        .read(record_offset(index), &mut bytes)
        .then_some(())?;
    StoredRecord::from_bytes(&bytes)
}

fn read_previous_record(storage: &mut impl StoreStorage, index: usize) -> Option<PreviousRecord> {
    let mut record = [0u8; PREVIOUS_RECORD_LEN];
    storage
        .read(record_offset(index), &mut record)
        .then_some(())?;
    if record.iter().all(|&byte| byte == 0xFF)
        || u32::from_le_bytes(record[0..4].try_into().ok()?) != APP_STATE_STORE_MAGIC
        || record[4] != PREVIOUS_VERSION
        || crc32(&record[..PREVIOUS_CRC_OFFSET])
            != u32::from_le_bytes(record[PREVIOUS_CRC_OFFSET..].try_into().ok()?)
    {
        return None;
    }
    Some(PreviousRecord {
        state: PersistedAppState {
            services: ServiceFlags::from_bits(record[5]),
            diag_kind: DiagKind::from_u8(record[6])?,
            diag_targets: DiagTargets::from_persisted(record[7]),
        },
        generation: u32::from_le_bytes(record[8..12].try_into().ok()?),
    })
}

fn write_record(storage: &mut impl StoreStorage, index: usize, record: &StoredRecord) -> bool {
    let offset = record_offset(index);
    let bytes = record.to_bytes();
    if !storage.replace(offset, &bytes) {
        return false;
    }
    let mut verify = [0u8; APP_STATE_STORE_RECORD_LEN];
    storage.read(offset, &mut verify) && StoredRecord::from_bytes(&verify).as_ref() == Some(record)
}

fn encode_ui_settings(
    record: &mut [u8; APP_STATE_STORE_RECORD_LEN],
    settings: &PersistedUiSettings,
) {
    let mut flags = 0;
    if settings.ambient_binding.is_some() {
        flags |= SETTINGS_AMBIENT_PRESENT;
    }
    if settings.startup_entry.is_some() {
        flags |= SETTINGS_STARTUP_ENTRY_PRESENT;
    }
    if settings.enablement_configured {
        flags |= SETTINGS_ENABLEMENT_CONFIGURED;
    }
    if settings.startup_overlays_configured {
        flags |= SETTINGS_STARTUP_OVERLAYS_CONFIGURED;
    }
    record[SETTINGS_FLAGS_OFFSET] = flags;
    record[SETTINGS_PIN_COUNT_OFFSET] = settings.pins.len() as u8;
    record[SETTINGS_ENABLED_COUNT_OFFSET] = settings.enabled_overlays.len() as u8;
    record[SETTINGS_STARTUP_OVERLAY_COUNT_OFFSET] = settings.startup_overlays.len() as u8;
    if let Some(id) = settings.ambient_binding {
        encode_id(record, SETTINGS_AMBIENT_OFFSET, id);
    }
    if let Some(id) = settings.startup_entry {
        encode_id(record, SETTINGS_STARTUP_ENTRY_OFFSET, id);
    }
    encode_ids(record, SETTINGS_PINS_OFFSET, &settings.pins);
    encode_ids(record, SETTINGS_ENABLED_OFFSET, &settings.enabled_overlays);
    encode_ids(
        record,
        SETTINGS_STARTUP_OVERLAYS_OFFSET,
        &settings.startup_overlays,
    );
}

fn decode_ui_settings(record: &[u8; APP_STATE_STORE_RECORD_LEN]) -> Option<PersistedUiSettings> {
    let flags = record[SETTINGS_FLAGS_OFFSET];
    if flags & !SETTINGS_SUPPORTED_FLAGS != 0 || record[120..124] != [0xFF; 4] {
        return None;
    }
    let pin_count = usize::from(record[SETTINGS_PIN_COUNT_OFFSET]);
    let enabled_count = usize::from(record[SETTINGS_ENABLED_COUNT_OFFSET]);
    let startup_overlay_count = usize::from(record[SETTINGS_STARTUP_OVERLAY_COUNT_OFFSET]);
    if pin_count > UI_SETTINGS_CAPACITY
        || enabled_count > UI_SETTINGS_CAPACITY
        || startup_overlay_count > UI_SETTINGS_CAPACITY
    {
        return None;
    }
    Some(PersistedUiSettings {
        ambient_binding: (flags & SETTINGS_AMBIENT_PRESENT != 0)
            .then(|| decode_id(record, SETTINGS_AMBIENT_OFFSET)),
        pins: decode_ids(record, SETTINGS_PINS_OFFSET, pin_count)?,
        enabled_overlays: decode_ids(record, SETTINGS_ENABLED_OFFSET, enabled_count)?,
        startup_entry: (flags & SETTINGS_STARTUP_ENTRY_PRESENT != 0)
            .then(|| decode_id(record, SETTINGS_STARTUP_ENTRY_OFFSET)),
        startup_overlays: decode_ids(
            record,
            SETTINGS_STARTUP_OVERLAYS_OFFSET,
            startup_overlay_count,
        )?,
        enablement_configured: flags & SETTINGS_ENABLEMENT_CONFIGURED != 0,
        startup_overlays_configured: flags & SETTINGS_STARTUP_OVERLAYS_CONFIGURED != 0,
    })
}

fn encode_ids(record: &mut [u8; APP_STATE_STORE_RECORD_LEN], offset: usize, ids: &[EntryId]) {
    for (index, id) in ids.iter().copied().enumerate() {
        encode_id(record, offset + index * SETTINGS_ID_LEN, id);
    }
}

fn decode_ids(
    record: &[u8; APP_STATE_STORE_RECORD_LEN],
    offset: usize,
    count: usize,
) -> Option<Vec<EntryId, UI_SETTINGS_CAPACITY>> {
    let mut ids = Vec::new();
    for index in 0..count {
        ids.push(decode_id(record, offset + index * SETTINGS_ID_LEN))
            .ok()?;
    }
    Some(ids)
}

fn encode_id(record: &mut [u8; APP_STATE_STORE_RECORD_LEN], offset: usize, id: EntryId) {
    record[offset..offset + 2].copy_from_slice(&id.namespace.to_le_bytes());
    record[offset + 2..offset + SETTINGS_ID_LEN].copy_from_slice(&id.local.to_le_bytes());
}

fn decode_id(record: &[u8; APP_STATE_STORE_RECORD_LEN], offset: usize) -> EntryId {
    EntryId::new(
        u16::from_le_bytes([record[offset], record[offset + 1]]),
        u16::from_le_bytes([record[offset + 2], record[offset + 3]]),
    )
}

fn read_legacy_record(storage: &mut impl StoreStorage) -> Option<(u8, PersistedAppState)> {
    let mut record = [0u8; LEGACY_RECORD_LEN];
    storage
        .read(APP_STATE_LEGACY_OFFSET, &mut record)
        .then_some(())?;
    decode_legacy_record(&record)
}

fn decode_legacy_record(record: &[u8; LEGACY_RECORD_LEN]) -> Option<(u8, PersistedAppState)> {
    let version = record[4];
    if record.iter().all(|&byte| byte == 0xFF)
        || u32::from_le_bytes(record[0..4].try_into().ok()?) != APP_STATE_STORE_MAGIC
        || !(LEGACY_VERSION_MIN..=LEGACY_VERSION_MAX).contains(&version)
        || checksum8(&record[..LEGACY_RECORD_LEN - 1]) != record[LEGACY_RECORD_LEN - 1]
    {
        return None;
    }
    Some((
        version,
        PersistedAppState {
            services: ServiceFlags::from_bits(record[5]),
            diag_kind: DiagKind::from_u8(record[6])?,
            diag_targets: DiagTargets::from_persisted(record[7]),
        },
    ))
}

fn legacy_state_for_migration(version: u8, state: PersistedAppState) -> PersistedAppState {
    if version == 2 {
        PersistedAppState::default()
    } else {
        state
    }
}

fn generation_is_newer(candidate: u32, baseline: u32) -> bool {
    candidate != baseline && candidate.wrapping_sub(baseline) < (u32::MAX / 2)
}

fn checksum8(bytes: &[u8]) -> u8 {
    let mut acc = 0x5Au8;
    for &byte in bytes {
        acc ^= byte.rotate_left(1);
    }
    acc
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

#[cfg(all(test, not(target_os = "none")))]
#[path = "store/tests.rs"]
mod tests;
