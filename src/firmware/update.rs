use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration, Timer};
use esp_bootloader_esp_idf::{
    ota::{Ota, OtaImageState},
    partitions::{self, AppPartitionSubType, DataPartitionSubType, PartitionType},
};

use super::flash;

include!(concat!(env!("OUT_DIR"), "/ota_build_config.rs"));

const OTA_DATA_OFFSET: u32 = 0xf000;
const OTA_DATA_SECTOR_SIZE: u32 = 0x1000;
// Single-production layout (ADR-0014, config/partitions-single-production.csv) — the only
// accepted shape since Phase 5 removed the two-slot A/B layout. Referenced by
// validate_layout()'s shape check below.
const SINGLE_PRODUCTION_FACTORY_OFFSET: u32 = 0x20000;
const SINGLE_PRODUCTION_FACTORY_SIZE: u32 = 0x60000;
const SINGLE_PRODUCTION_OTA_0_OFFSET: u32 = 0x80000;
const SINGLE_PRODUCTION_OTA_0_SIZE: u32 = 0x380000;

static TRANSPORT_QUIET: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Ota0,
    /// The single-production layout's factory (updater) partition
    /// (ADR-0014). Booted-partition reporting needs this — the factory
    /// updater's own `status()` call resolves here when it's running from
    /// `factory`.
    Factory,
}

impl Slot {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Ota0 => "ota_0",
            Self::Factory => "factory",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Status {
    pub(crate) build_id: &'static str,
    pub(crate) booted: Slot,
    pub(crate) selected: Option<Slot>,
    pub(crate) image_state: Option<OtaImageState>,
    pub(crate) public_key_configured: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpdateError {
    Layout,
    Flash,
    Metadata,
}

impl UpdateError {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Layout => "layout",
            Self::Flash => "flash",
            Self::Metadata => "metadata",
        }
    }
}

pub(crate) fn initialize_boot_state() -> Result<Status, UpdateError> {
    validate_layout()?;
    let initial_status = status()?;
    if initial_status.selected.is_none() {
        write_selection(initial_status.booted, OtaImageState::Valid)?;
    }
    let status = status()?;
    console::println!(
        "FIRMWARE_BOOT booted={} selected={} state={} key_configured={} build_id={}",
        status.booted.label(),
        status.selected.map_or("none", Slot::label),
        status.image_state.map_or("none", image_state_label),
        if status.public_key_configured {
            "yes"
        } else {
            "no"
        },
        status.build_id,
    );
    Ok(status)
}

pub(crate) fn status() -> Result<Status, UpdateError> {
    let (booted, selected, image_state) = read_boot_metadata()?;
    Ok(Status {
        build_id: OTA_BUILD_ID,
        booted,
        selected,
        image_state,
        public_key_configured: OTA_PUBLIC_KEY_CONFIGURED,
    })
}

pub(crate) fn transport_quiet() -> bool {
    TRANSPORT_QUIET.load(Ordering::Acquire)
}

pub(crate) fn end_transport() {
    TRANSPORT_QUIET.store(false, Ordering::Release);
}

pub(crate) fn confirm_pending_image() -> Result<bool, UpdateError> {
    let (_, selected, state) = read_boot_metadata()?;
    if state != Some(OtaImageState::PendingVerify) {
        return Ok(false);
    }
    let selected = selected.ok_or(UpdateError::Metadata)?;
    write_current_state(OtaImageState::Valid)?;
    console::println!("FIRMWARE_CONFIRM slot={} state=valid", selected.label());
    Ok(true)
}

/// Selects the factory (updater) partition for the next boot by erasing
/// both `otadata` records — `Ota::set_current_app_partition(Factory)`
/// (ADR-0014's "factory selection erases both OTA records"). The bootloader
/// then boots `factory` unconditionally on the next reset, independent of
/// whatever `ota_0`'s prior sequence/state was: with both records erased
/// (`ota_seq == 0xFFFFFFFF`, "invalid" per `bootloader_common_ota_select_valid`),
/// `bootloader_utility_get_selected_boot_partition` falls back to the
/// factory partition — the same stock ESP-IDF mechanism that already
/// handles "reset before confirmation" (see docs/architecture/0014-single-production-sd-recovery-updater.md).
/// Refuses (`UpdateError::Layout`) if the device has no `factory` partition
/// to fall back to (e.g. a board not yet migrated off an old A/B image),
/// rather than erasing `otadata` out from under a layout with nothing to
/// recover to.
///
/// Callable from either binary that links this module: `src/updater/`
/// (Phase 3's `install::run`) and the default `meditamer` binary's serial
/// dispatch (Phase 4, an operator-triggered recovery request — see
/// `SerialCommand::FirmwareFactoryBoot`).
pub(crate) fn request_factory_boot() -> Result<(), UpdateError> {
    flash::with(|flash| {
        let mut buffer = [0u8; partitions::PARTITION_TABLE_MAX_LEN];
        let table = partitions::read_partition_table(flash, &mut buffer)
            .map_err(|_| UpdateError::Layout)?;
        let has_factory = table
            .find_partition(PartitionType::App(AppPartitionSubType::Factory))
            .ok()
            .flatten()
            .is_some();
        if !has_factory {
            return Err(UpdateError::Layout);
        }
        let ota_entry = table
            .find_partition(PartitionType::Data(DataPartitionSubType::Ota))
            .map_err(|_| UpdateError::Metadata)?
            .ok_or(UpdateError::Metadata)?;
        let mut ota =
            Ota::new(ota_entry.as_embedded_storage(flash), 1).map_err(|_| UpdateError::Metadata)?;
        ota.set_current_app_partition(AppPartitionSubType::Factory)
            .map_err(|_| UpdateError::Metadata)?;
        console::println!("FIRMWARE_FACTORY_REQUEST state=erased");
        Ok(())
    })
}

pub(crate) const fn image_state_label(state: OtaImageState) -> &'static str {
    match state {
        OtaImageState::New => "new",
        OtaImageState::PendingVerify => "pending_verify",
        OtaImageState::Valid => "valid",
        OtaImageState::Invalid => "invalid",
        OtaImageState::Aborted => "aborted",
        OtaImageState::Undefined => "undefined",
    }
}

/// The single-production layout (`config/partitions-single-production.csv`)
/// is the only accepted shape since ADR-0014 Phase 5 removed the two-slot
/// A/B layout.
fn validate_layout() -> Result<(), UpdateError> {
    flash::with(|flash| {
        let mut buffer = [0u8; partitions::PARTITION_TABLE_MAX_LEN];
        let table = partitions::read_partition_table(flash, &mut buffer)
            .map_err(|_| UpdateError::Layout)?;
        let expected: &[(&str, u32, u32)] = &[
            ("otadata", OTA_DATA_OFFSET, 0x2000),
            ("app_state", 0x12000, 0x2000),
            (
                "factory",
                SINGLE_PRODUCTION_FACTORY_OFFSET,
                SINGLE_PRODUCTION_FACTORY_SIZE,
            ),
            (
                "ota_0",
                SINGLE_PRODUCTION_OTA_0_OFFSET,
                SINGLE_PRODUCTION_OTA_0_SIZE,
            ),
        ];
        for (label, offset, len) in expected.iter().copied() {
            let found = table
                .iter()
                .find(|entry| entry.label_as_str() == label)
                .ok_or(UpdateError::Layout)?;
            if found.offset() != offset || found.len() != len {
                return Err(UpdateError::Layout);
            }
        }
        Ok(())
    })
}

fn read_boot_metadata() -> Result<(Slot, Option<Slot>, Option<OtaImageState>), UpdateError> {
    flash::with(|flash| {
        let mut buffer = [0u8; partitions::PARTITION_TABLE_MAX_LEN];
        let table = partitions::read_partition_table(flash, &mut buffer)
            .map_err(|_| UpdateError::Layout)?;
        let booted = table
            .booted_partition()
            .map_err(|_| UpdateError::Metadata)?
            .and_then(|entry| slot_from_subtype(entry.partition_type()))
            .ok_or(UpdateError::Metadata)?;
        let ota_entry = table
            .find_partition(PartitionType::Data(DataPartitionSubType::Ota))
            .map_err(|_| UpdateError::Metadata)?
            .ok_or(UpdateError::Metadata)?;
        let mut ota =
            Ota::new(ota_entry.as_embedded_storage(flash), 1).map_err(|_| UpdateError::Metadata)?;
        let selected = ota.current_app_partition().ok().and_then(slot_from_app);
        let state = ota.current_ota_state().ok();
        Ok((booted, selected, state))
    })
}

fn write_current_state(state: OtaImageState) -> Result<(), UpdateError> {
    flash::with(|flash| {
        let mut buffer = [0u8; partitions::PARTITION_TABLE_MAX_LEN];
        let table = partitions::read_partition_table(flash, &mut buffer)
            .map_err(|_| UpdateError::Layout)?;
        let ota_entry = table
            .find_partition(PartitionType::Data(DataPartitionSubType::Ota))
            .map_err(|_| UpdateError::Metadata)?
            .ok_or(UpdateError::Metadata)?;
        let mut ota =
            Ota::new(ota_entry.as_embedded_storage(flash), 1).map_err(|_| UpdateError::Metadata)?;
        ota.set_current_ota_state(state)
            .map_err(|_| UpdateError::Metadata)
    })
}

/// `otadata` holds two redundant sector records so a torn write never loses
/// both copies; this rotates between them and bumps the sequence number,
/// independent of which (sole, since Phase 5) app partition is selected —
/// with only one app partition, any sequence value resolves to it.
fn write_selection(target: Slot, state: OtaImageState) -> Result<(), UpdateError> {
    // write_selection is only ever called with a booted slot from the
    // meditamer production binary (initialize_boot_state()), which never
    // runs from `factory` — that's the separate updater binary. Reaching
    // this with a non-Ota0 target means something called it from an
    // unexpected context; fail closed rather than write a nonsensical
    // selection.
    if target != Slot::Ota0 {
        return Err(UpdateError::Layout);
    }
    let mut first = [0u8; 32];
    let mut second = [0u8; 32];
    flash::read(OTA_DATA_OFFSET, &mut first).map_err(|_| UpdateError::Flash)?;
    flash::read(OTA_DATA_OFFSET + OTA_DATA_SECTOR_SIZE, &mut second)
        .map_err(|_| UpdateError::Flash)?;
    let seq0 = valid_ota_sequence(&first);
    let seq1 = valid_ota_sequence(&second);
    let maximum = match (seq0, seq1) {
        (Some(a), Some(b)) => a.max(b),
        (Some(value), None) | (None, Some(value)) => value,
        (None, None) => 0,
    };
    let sequence = maximum.saturating_add(1).max(1);
    let target_sector = match (seq0, seq1) {
        (None, _) => 0,
        (_, None) => 1,
        (Some(a), Some(b)) if a <= b => 0,
        (Some(_), Some(_)) => 1,
    };
    let mut entry = [0xffu8; 32];
    entry[0..4].copy_from_slice(&sequence.to_le_bytes());
    entry[24..28].copy_from_slice(&(state as u32).to_le_bytes());
    entry[28..32].copy_from_slice(&ota_crc(sequence).to_le_bytes());
    flash::replace(
        OTA_DATA_OFFSET + target_sector * OTA_DATA_SECTOR_SIZE,
        &entry,
    )
    .map_err(|_| UpdateError::Metadata)?;
    let (_, selected, selected_state) = read_boot_metadata()?;
    if selected != Some(target) || selected_state != Some(state) {
        return Err(UpdateError::Metadata);
    }
    Ok(())
}

fn valid_ota_sequence(entry: &[u8; 32]) -> Option<u32> {
    let sequence = u32::from_le_bytes(entry[0..4].try_into().ok()?);
    if sequence == u32::MAX
        || u32::from_le_bytes(entry[28..32].try_into().ok()?) != ota_crc(sequence)
    {
        return None;
    }
    Some(sequence)
}

// Extracted to the host-testable `otadata` crate (ADR-0014 Phase 5) since this
// crate itself is `[lib] test = false`; see that crate's doc comment.
pub(crate) use otadata::ota_crc;

fn slot_from_subtype(partition_type: PartitionType) -> Option<Slot> {
    match partition_type {
        PartitionType::App(subtype) => slot_from_app(subtype),
        _ => None,
    }
}

fn slot_from_app(subtype: AppPartitionSubType) -> Option<Slot> {
    match subtype {
        AppPartitionSubType::Ota0 => Some(Slot::Ota0),
        AppPartitionSubType::Factory => Some(Slot::Factory),
        _ => None,
    }
}

#[embassy_executor::task]
pub(crate) async fn firmware_health_task() {
    let pending = status()
        .map(|status| status.image_state == Some(OtaImageState::PendingVerify))
        .unwrap_or(false);
    if !pending {
        return;
    }

    if option_env!("MEDITAMER_FIRMWARE_SKIP_CONFIRMATION").is_some() {
        console::println!(
            "FIRMWARE_HEALTH state=pending gate=confirmation_withheld build_fixture=yes"
        );
        return;
    }

    console::println!("FIRMWARE_HEALTH state=pending gate=runtime_ready_plus_5000ms");
    while !crate::firmware::scheduling::runtime_ready() {
        Timer::after(Duration::from_millis(50)).await;
    }
    Timer::after(Duration::from_secs(5)).await;
    if let Err(error) = confirm_pending_image() {
        console::println!("FIRMWARE_CONFIRM status=error reason={}", error.label());
    }
}
