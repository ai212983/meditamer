use core::{
    cell::RefCell,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use critical_section::Mutex;
use ed25519_dalek::{Signature, VerifyingKey};
use esp_bootloader_esp_idf::{
    ota::{Ota, OtaImageState},
    partitions::{self, AppPartitionSubType, DataPartitionSubType, PartitionType},
};
use sha2::{Digest, Sha256};

use super::{app_state, flash};

include!(concat!(env!("OUT_DIR"), "/ota_build_config.rs"));

// Legacy hex encoding plus command framing must fit in the ESP32 UART0 receive FIFO. The binary
// stream is framed separately and sends one flash-sized payload at a time.
pub(crate) const LEGACY_CHUNK_MAX: usize = 48;
pub(crate) const STREAM_CHUNK_MAX: usize = 112;
// Keep ROM flash calls no larger than the 256-byte size already proven on this board. Larger
// calls can stall even when their source is correctly placed in internal RAM.
const FLASH_BATCH_BYTES: usize = STREAM_CHUNK_MAX * 2;
const OTA_DATA_OFFSET: u32 = 0xf000;
const OTA_DATA_SECTOR_SIZE: u32 = 0x1000;
const OTA_0_OFFSET: u32 = 0x20000;
const OTA_1_OFFSET: u32 = 0x210000;
const OTA_SLOT_SIZE: u32 = 0x1f0000;
const OTA_MIN_HEADROOM: u32 = 0x20000;
const FLASH_PROGRAM_PAGE_BYTES: u32 = 256;
const IMAGE_HEADER_LEN: usize = 24;
const IMAGE_SEGMENT_HEADER_LEN: u32 = 8;
const ESP_IMAGE_MAGIC: u8 = 0xe9;
const ESP_IMAGE_MAX_SEGMENTS: u8 = 16;
const SIGNING_DOMAIN: &[u8] = b"MEDITAMER-FIRMWARE-V1";

static UPDATE_SESSION: Mutex<RefCell<Option<UpdateSession>>> = Mutex::new(RefCell::new(None));
static TRANSPORT_QUIET: AtomicBool = AtomicBool::new(false);
static MAX_ERASE_US: AtomicU32 = AtomicU32::new(0);
static MAX_WRITE_US: AtomicU32 = AtomicU32::new(0);
static LAST_VERIFY_READ_US: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Ota0,
    Ota1,
}

impl Slot {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Ota0 => "ota_0",
            Self::Ota1 => "ota_1",
        }
    }

    const fn offset(self) -> u32 {
        match self {
            Self::Ota0 => OTA_0_OFFSET,
            Self::Ota1 => OTA_1_OFFSET,
        }
    }

    const fn opposite(self) -> Self {
        match self {
            Self::Ota0 => Self::Ota1,
            Self::Ota1 => Self::Ota0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionPhase {
    Idle,
    Receiving,
    Verified,
}

impl SessionPhase {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Receiving => "receiving",
            Self::Verified => "verified",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Status {
    pub(crate) build_id: &'static str,
    pub(crate) booted: Slot,
    pub(crate) selected: Option<Slot>,
    pub(crate) image_state: Option<OtaImageState>,
    pub(crate) phase: SessionPhase,
    pub(crate) target: Option<Slot>,
    pub(crate) written: u32,
    pub(crate) total: u32,
    pub(crate) public_key_configured: bool,
    pub(crate) public_key_id: [u8; 4],
    pub(crate) max_erase_us: u32,
    pub(crate) max_write_us: u32,
    pub(crate) verify_read_us: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpdateError {
    Layout,
    StateMigration,
    SigningKey,
    Busy,
    NoSession,
    WrongPhase,
    InvalidLength,
    InvalidOffset,
    InvalidChunk,
    InvalidImage,
    DigestMismatch,
    Signature,
    Flash,
    Metadata,
}

impl UpdateError {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Layout => "layout",
            Self::StateMigration => "state_migration",
            Self::SigningKey => "signing_key",
            Self::Busy => "busy",
            Self::NoSession => "no_session",
            Self::WrongPhase => "wrong_phase",
            Self::InvalidLength => "invalid_length",
            Self::InvalidOffset => "invalid_offset",
            Self::InvalidChunk => "invalid_chunk",
            Self::InvalidImage => "invalid_image",
            Self::DigestMismatch => "digest_mismatch",
            Self::Signature => "signature",
            Self::Flash => "flash",
            Self::Metadata => "metadata",
        }
    }
}

struct UpdateSession {
    phase: SessionPhase,
    target: Slot,
    image_len: u32,
    written: u32,
    flash_written: u32,
    erased_until: u32,
    buffered: [u8; FLASH_BATCH_BYTES],
    buffered_len: usize,
    last_chunk_offset: u32,
    last_chunk: [u8; STREAM_CHUNK_MAX],
    last_chunk_len: u16,
    expected_digest: [u8; 32],
    signature: [u8; 64],
    hasher: Sha256,
}

#[repr(align(4))]
struct InternalFlashBatch([u8; FLASH_BATCH_BYTES]);

pub(crate) fn initialize_boot_state() -> Result<Status, UpdateError> {
    validate_layout()?;
    let initial_status = status()?;
    if initial_status.selected.is_none() {
        write_selection(initial_status.booted, OtaImageState::Valid)?;
    }
    let status = status()?;
    esp_println::println!(
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
    let (phase, target, written, total) = critical_section::with(|cs| {
        let session = UPDATE_SESSION.borrow_ref(cs);
        session
            .as_ref()
            .map_or((SessionPhase::Idle, None, 0, 0), |session| {
                (
                    session.phase,
                    Some(session.target),
                    session.written,
                    session.image_len,
                )
            })
    });
    Ok(Status {
        build_id: OTA_BUILD_ID,
        booted,
        selected,
        image_state,
        phase,
        target,
        written,
        total,
        public_key_configured: OTA_PUBLIC_KEY_CONFIGURED,
        public_key_id: public_key_id(),
        max_erase_us: MAX_ERASE_US.load(Ordering::Relaxed),
        max_write_us: MAX_WRITE_US.load(Ordering::Relaxed),
        verify_read_us: LAST_VERIFY_READ_US.load(Ordering::Relaxed),
    })
}

pub(crate) fn prepare_transport() {
    TRANSPORT_QUIET.store(true, Ordering::Release);
}

pub(crate) fn transport_quiet() -> bool {
    TRANSPORT_QUIET.load(Ordering::Acquire)
}

fn public_key_id() -> [u8; 4] {
    if !OTA_PUBLIC_KEY_CONFIGURED {
        return [0; 4];
    }
    let digest: [u8; 32] = Sha256::digest(OTA_PUBLIC_KEY).into();
    digest[..4].try_into().unwrap()
}

pub(crate) fn begin(
    image_len: u32,
    expected_digest: [u8; 32],
    signature: [u8; 64],
) -> Result<Slot, UpdateError> {
    validate_layout()?;
    if !app_state::store::migration_complete() {
        return Err(UpdateError::StateMigration);
    }
    if !OTA_PUBLIC_KEY_CONFIGURED {
        return Err(UpdateError::SigningKey);
    }
    if !(256..=OTA_SLOT_SIZE - OTA_MIN_HEADROOM).contains(&image_len)
        || !image_len.is_multiple_of(4)
    {
        return Err(UpdateError::InvalidLength);
    }
    let booted = read_boot_metadata()?.0;
    let target = booted.opposite();
    MAX_ERASE_US.store(0, Ordering::Relaxed);
    MAX_WRITE_US.store(0, Ordering::Relaxed);
    LAST_VERIFY_READ_US.store(0, Ordering::Relaxed);
    critical_section::with(|cs| {
        let mut session = UPDATE_SESSION.borrow_ref_mut(cs);
        if session.is_some() {
            return Err(UpdateError::Busy);
        }
        *session = Some(UpdateSession {
            phase: SessionPhase::Receiving,
            target,
            image_len,
            written: 0,
            flash_written: 0,
            erased_until: 0,
            buffered: [0; FLASH_BATCH_BYTES],
            buffered_len: 0,
            last_chunk_offset: 0,
            last_chunk: [0; STREAM_CHUNK_MAX],
            last_chunk_len: 0,
            expected_digest,
            signature,
            hasher: Sha256::new(),
        });
        Ok(target)
    })
}

pub(crate) fn write_chunk(offset: u32, bytes: &[u8]) -> Result<u32, UpdateError> {
    if bytes.is_empty() || bytes.len() > STREAM_CHUNK_MAX || !bytes.len().is_multiple_of(4) {
        return Err(UpdateError::InvalidChunk);
    }
    let mut internal = InternalFlashBatch([0; FLASH_BATCH_BYTES]);
    let (target, flash_offset, flash_len, erase_from, erase_to, accepted) =
        critical_section::with(|cs| {
            let mut session = UPDATE_SESSION.borrow_ref_mut(cs);
            let session = session.as_mut().ok_or(UpdateError::NoSession)?;
            if session.phase != SessionPhase::Receiving {
                return Err(UpdateError::WrongPhase);
            }
            if offset != session.written {
                if session.last_chunk_offset == offset
                    && session.last_chunk_len as usize == bytes.len()
                    && &session.last_chunk[..bytes.len()] == bytes
                {
                    return Ok((session.target, 0, 0, 0, 0, session.written));
                }
                return Err(UpdateError::InvalidOffset);
            }
            if offset.saturating_add(bytes.len() as u32) > session.image_len {
                return Err(UpdateError::InvalidOffset);
            }
            if offset == 0 && !valid_image_header(bytes) {
                return Err(UpdateError::InvalidImage);
            }

            let buffered_end = session.buffered_len + bytes.len();
            if buffered_end > session.buffered.len() {
                return Err(UpdateError::InvalidChunk);
            }
            session.buffered[session.buffered_len..buffered_end].copy_from_slice(bytes);
            session.buffered_len = buffered_end;
            session.hasher.update(bytes);
            session.written += bytes.len() as u32;

            let flush = session.buffered_len == session.buffered.len()
                || session.written == session.image_len;
            if !flush {
                remember_last_chunk(session, offset, bytes);
                return Ok((session.target, 0, 0, 0, 0, session.written));
            }

            let flash_offset = session.flash_written;
            let flash_len = session.buffered_len;
            internal.0[..flash_len].copy_from_slice(&session.buffered[..flash_len]);
            let needed = align_up(flash_offset + flash_len as u32, OTA_DATA_SECTOR_SIZE);
            let erase_from = session.erased_until;
            let erase_to = needed.min(OTA_SLOT_SIZE);
            Ok((
                session.target,
                flash_offset,
                flash_len,
                erase_from,
                erase_to,
                session.written,
            ))
        })?;

    if flash_len == 0 {
        return Ok(accepted);
    }

    if erase_to > erase_from {
        let started_us = embassy_time::Instant::now().as_micros();
        flash::erase(target.offset() + erase_from, target.offset() + erase_to)
            .map_err(|_| UpdateError::Flash)?;
        record_max(
            &MAX_ERASE_US,
            embassy_time::Instant::now()
                .as_micros()
                .saturating_sub(started_us),
        );
    }
    // Serial commands are handled in a boxed future and the global allocator can place that
    // future in external PSRAM. The ROM flash routine disables the cache and therefore cannot
    // safely consume its input directly from that future. Coalesce accepted wire chunks and copy
    // each batch to aligned internal stack RAM before entering the flash driver.
    let mut batch_written = 0usize;
    while batch_written < flash_len {
        let absolute_offset = flash_offset + batch_written as u32;
        let page_remaining = FLASH_PROGRAM_PAGE_BYTES - absolute_offset % FLASH_PROGRAM_PAGE_BYTES;
        let segment_len = (flash_len - batch_written).min(page_remaining as usize);
        let started_us = embassy_time::Instant::now().as_micros();
        flash::write(
            target.offset() + absolute_offset,
            &internal.0[batch_written..batch_written + segment_len],
        )
        .map_err(|_| UpdateError::Flash)?;
        record_max(
            &MAX_WRITE_US,
            embassy_time::Instant::now()
                .as_micros()
                .saturating_sub(started_us),
        );
        batch_written += segment_len;
    }

    critical_section::with(|cs| {
        let mut session = UPDATE_SESSION.borrow_ref_mut(cs);
        let session = session.as_mut().ok_or(UpdateError::NoSession)?;
        if session.flash_written != flash_offset || session.target != target {
            return Err(UpdateError::InvalidOffset);
        }
        session.flash_written += flash_len as u32;
        session.buffered_len = 0;
        session.erased_until = session.erased_until.max(erase_to);
        remember_last_chunk(session, offset, bytes);
        Ok(accepted)
    })
}

fn remember_last_chunk(session: &mut UpdateSession, offset: u32, bytes: &[u8]) {
    session.last_chunk_offset = offset;
    session.last_chunk[..bytes.len()].copy_from_slice(bytes);
    session.last_chunk_len = bytes.len() as u16;
}

pub(crate) fn prepare_stream() -> Result<u32, UpdateError> {
    let (target, mut erased_until, erase_to) = critical_section::with(|cs| {
        let session = UPDATE_SESSION.borrow_ref(cs);
        let session = session.as_ref().ok_or(UpdateError::NoSession)?;
        if session.phase != SessionPhase::Receiving || session.written != 0 {
            return Err(UpdateError::WrongPhase);
        }
        Ok((
            session.target,
            session.erased_until,
            align_up(session.image_len, OTA_DATA_SECTOR_SIZE).min(OTA_SLOT_SIZE),
        ))
    })?;

    while erased_until < erase_to {
        let next = (erased_until + OTA_DATA_SECTOR_SIZE).min(erase_to);
        let started_us = embassy_time::Instant::now().as_micros();
        flash::erase(target.offset() + erased_until, target.offset() + next)
            .map_err(|_| UpdateError::Flash)?;
        record_max(
            &MAX_ERASE_US,
            embassy_time::Instant::now()
                .as_micros()
                .saturating_sub(started_us),
        );
        critical_section::with(|cs| {
            let mut session = UPDATE_SESSION.borrow_ref_mut(cs);
            let session = session.as_mut().ok_or(UpdateError::NoSession)?;
            if session.target != target || session.written != 0 {
                return Err(UpdateError::InvalidOffset);
            }
            session.erased_until = session.erased_until.max(next);
            Ok(())
        })?;
        erased_until = next;
    }
    Ok(erase_to)
}

pub(crate) fn stream_complete() -> bool {
    critical_section::with(|cs| {
        UPDATE_SESSION
            .borrow_ref(cs)
            .as_ref()
            .is_some_and(|session| session.written == session.image_len)
    })
}

pub(crate) fn finish() -> Result<[u8; 32], UpdateError> {
    let (target, image_len, expected, signature, digest) = critical_section::with(|cs| {
        let session = UPDATE_SESSION.borrow_ref(cs);
        let session = session.as_ref().ok_or(UpdateError::NoSession)?;
        if session.phase != SessionPhase::Receiving {
            return Err(UpdateError::WrongPhase);
        }
        if session.written != session.image_len
            || session.flash_written != session.image_len
            || session.buffered_len != 0
        {
            return Err(UpdateError::InvalidLength);
        }
        let digest: [u8; 32] = session.hasher.clone().finalize().into();
        Ok((
            session.target,
            session.image_len,
            session.expected_digest,
            session.signature,
            digest,
        ))
    })?;

    if digest != expected {
        return Err(UpdateError::DigestMismatch);
    }
    verify_signature(image_len, &digest, &signature)?;
    let verify_started_us = embassy_time::Instant::now().as_micros();
    let staged_digest = digest_staged_image(target, image_len)?;
    let verify_read_us = embassy_time::Instant::now()
        .as_micros()
        .saturating_sub(verify_started_us)
        .min(u32::MAX as u64) as u32;
    LAST_VERIFY_READ_US.store(verify_read_us, Ordering::Relaxed);
    if staged_digest != expected {
        return Err(UpdateError::DigestMismatch);
    }
    validate_staged_image(target, image_len)?;

    esp_println::println!(
        "FIRMWARE_STAGE target={} bytes={} erase_max_us={} write_max_us={} verify_read_us={} multicore=transaction_park",
        target.label(),
        image_len,
        MAX_ERASE_US.load(Ordering::Relaxed),
        MAX_WRITE_US.load(Ordering::Relaxed),
        verify_read_us,
    );

    critical_section::with(|cs| {
        let mut session = UPDATE_SESSION.borrow_ref_mut(cs);
        let session = session.as_mut().ok_or(UpdateError::NoSession)?;
        session.phase = SessionPhase::Verified;
        Ok(digest)
    })
}

fn digest_staged_image(target: Slot, image_len: u32) -> Result<[u8; 32], UpdateError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; LEGACY_CHUNK_MAX];
    let mut offset = 0u32;
    while offset < image_len {
        let remaining = (image_len - offset) as usize;
        let len = remaining.min(buffer.len());
        flash::read_aligned(target.offset() + offset, &mut buffer[..len])
            .map_err(|_| UpdateError::Flash)?;
        hasher.update(&buffer[..len]);
        offset += len as u32;
    }
    Ok(hasher.finalize().into())
}

fn record_max(value: &AtomicU32, measurement_us: u64) {
    let measurement = measurement_us.min(u32::MAX as u64) as u32;
    value.fetch_max(measurement, Ordering::Relaxed);
}

pub(crate) fn activate() -> Result<Slot, UpdateError> {
    let target = critical_section::with(|cs| {
        let session = UPDATE_SESSION.borrow_ref(cs);
        let session = session.as_ref().ok_or(UpdateError::NoSession)?;
        if session.phase != SessionPhase::Verified {
            return Err(UpdateError::WrongPhase);
        }
        Ok(session.target)
    })?;
    write_selection(target, OtaImageState::New)?;
    critical_section::with(|cs| *UPDATE_SESSION.borrow_ref_mut(cs) = None);
    Ok(target)
}

pub(crate) fn abort() {
    critical_section::with(|cs| *UPDATE_SESSION.borrow_ref_mut(cs) = None);
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
    esp_println::println!("FIRMWARE_CONFIRM slot={} state=valid", selected.label());
    Ok(true)
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

fn valid_image_header(bytes: &[u8]) -> bool {
    bytes.len() >= IMAGE_HEADER_LEN
        && bytes[0] == ESP_IMAGE_MAGIC
        && (1..=ESP_IMAGE_MAX_SEGMENTS).contains(&bytes[1])
        && u16::from_le_bytes([bytes[12], bytes[13]]) == 0
        && bytes[23] == 1
}

fn validate_staged_image(target: Slot, image_len: u32) -> Result<(), UpdateError> {
    let mut header = [0u8; IMAGE_HEADER_LEN];
    flash::read_aligned(target.offset(), &mut header).map_err(|_| UpdateError::Flash)?;
    if !valid_image_header(&header) {
        return Err(UpdateError::InvalidImage);
    }
    let mut cursor = IMAGE_HEADER_LEN as u32;
    for _ in 0..header[1] {
        let mut segment = [0u8; IMAGE_SEGMENT_HEADER_LEN as usize];
        flash::read_aligned(target.offset() + cursor, &mut segment)
            .map_err(|_| UpdateError::Flash)?;
        let segment_len = u32::from_le_bytes(segment[4..8].try_into().unwrap());
        if segment_len == 0 || !segment_len.is_multiple_of(4) {
            return Err(UpdateError::InvalidImage);
        }
        cursor = cursor
            .checked_add(IMAGE_SEGMENT_HEADER_LEN)
            .and_then(|value| value.checked_add(segment_len))
            .ok_or(UpdateError::InvalidImage)?;
        if cursor >= image_len {
            return Err(UpdateError::InvalidImage);
        }
    }
    let checksum_offset = cursor | 0x0f;
    let expected_len = checksum_offset
        .checked_add(1 + 32)
        .ok_or(UpdateError::InvalidImage)?;
    if expected_len != image_len {
        return Err(UpdateError::InvalidImage);
    }
    Ok(())
}

fn verify_signature(
    image_len: u32,
    digest: &[u8; 32],
    signature: &[u8; 64],
) -> Result<(), UpdateError> {
    let key = VerifyingKey::from_bytes(&OTA_PUBLIC_KEY).map_err(|_| UpdateError::SigningKey)?;
    let signature = Signature::from_bytes(signature);
    let mut message = [0u8; SIGNING_DOMAIN.len() + 4 + 32];
    message[..SIGNING_DOMAIN.len()].copy_from_slice(SIGNING_DOMAIN);
    message[SIGNING_DOMAIN.len()..SIGNING_DOMAIN.len() + 4]
        .copy_from_slice(&image_len.to_le_bytes());
    message[SIGNING_DOMAIN.len() + 4..].copy_from_slice(digest);
    key.verify_strict(&message, &signature)
        .map_err(|_| UpdateError::Signature)
}

fn validate_layout() -> Result<(), UpdateError> {
    flash::with(|flash| {
        let mut buffer = [0u8; partitions::PARTITION_TABLE_MAX_LEN];
        let table = partitions::read_partition_table(flash, &mut buffer)
            .map_err(|_| UpdateError::Layout)?;
        for (label, offset, len) in [
            ("otadata", OTA_DATA_OFFSET, 0x2000),
            ("app_state", 0x12000, 0x2000),
            ("ota_0", OTA_0_OFFSET, OTA_SLOT_SIZE),
            ("ota_1", OTA_1_OFFSET, OTA_SLOT_SIZE),
        ] {
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
            Ota::new(ota_entry.as_embedded_storage(flash), 2).map_err(|_| UpdateError::Metadata)?;
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
            Ota::new(ota_entry.as_embedded_storage(flash), 2).map_err(|_| UpdateError::Metadata)?;
        ota.set_current_ota_state(state)
            .map_err(|_| UpdateError::Metadata)
    })
}

fn write_selection(target: Slot, state: OtaImageState) -> Result<(), UpdateError> {
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
    let mut sequence = maximum.saturating_add(1).max(1);
    let required_parity = match target {
        Slot::Ota0 => 1,
        Slot::Ota1 => 0,
    };
    if sequence % 2 != required_parity {
        sequence = sequence.saturating_add(1);
    }
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

fn ota_crc(sequence: u32) -> u32 {
    let mut crc = 0u32;
    for byte in sequence.to_le_bytes() {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn slot_from_subtype(partition_type: PartitionType) -> Option<Slot> {
    match partition_type {
        PartitionType::App(subtype) => slot_from_app(subtype),
        _ => None,
    }
}

fn slot_from_app(subtype: AppPartitionSubType) -> Option<Slot> {
    match subtype {
        AppPartitionSubType::Ota0 => Some(Slot::Ota0),
        AppPartitionSubType::Ota1 => Some(Slot::Ota1),
        _ => None,
    }
}

const fn align_up(value: u32, alignment: u32) -> u32 {
    value.saturating_add(alignment - 1) & !(alignment - 1)
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    #[test]
    fn ota_crc_matches_esp_idf_examples() {
        assert_eq!(ota_crc(1), 0x4743_989a);
        assert_eq!(ota_crc(2), 0x55f6_3774);
        assert_eq!(ota_crc(3), 0xed4a_5011);
    }

    #[test]
    fn rejects_wrong_image_headers() {
        let mut header = [0u8; IMAGE_HEADER_LEN];
        header[0] = ESP_IMAGE_MAGIC;
        header[1] = 5;
        header[23] = 1;
        assert!(valid_image_header(&header));
        header[12] = 1;
        assert!(!valid_image_header(&header));
    }
}
