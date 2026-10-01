//! Versioned two-slot flash persistence for Wi-Fi credentials.
//!
//! Layout: two alternating sectors. Each sector holds one [`RECORD_LEN`]-byte
//! record at the sector base. `store` only erases/writes/verifies the inactive
//! (or older) slot, so a torn update leaves the prior record intact.

use super::types::{WifiCredentials, WIFI_PASSWORD_MAX, WIFI_SSID_MAX};

/// Number of alternating record slots.
pub const SLOT_COUNT: usize = 2;
/// Erase granularity of one slot.
pub const SECTOR_SIZE: u32 = 0x1000;
/// Total partition footprint (`SLOT_COUNT * SECTOR_SIZE`).
pub const PARTITION_SIZE: u32 = 0x2000;
/// Fixed on-flash record length.
pub const RECORD_LEN: usize = 128;

// Compile-time footprint sanity.
const _: () = assert!(SLOT_COUNT == 2);
const _: () = assert!((SLOT_COUNT as u32) * SECTOR_SIZE == PARTITION_SIZE);
const _: () = assert!(RECORD_LEN as u32 <= SECTOR_SIZE);

// Record field layout (all integers little-endian):
//   0..4   magic u32
//   4      version u8
//   5      ssid_len u8
//   6      password_len u8
//   7      reserved (0xFF)
//   8..12  generation u32
//   12..44 ssid payload ([u8; 32], full fixed array)
//   44..108 password payload ([u8; 64], full fixed array)
//   108..112 CRC32-IEEE over bytes 0..108
//   112..128 0xFF padding
const MAGIC: u32 = 0x5746_4331;
const VERSION: u8 = 1;
const OFF_VERSION: usize = 4;
const OFF_SSID_LEN: usize = 5;
const OFF_PASSWORD_LEN: usize = 6;
const OFF_RESERVED: usize = 7;
const OFF_GENERATION: usize = 8;
const OFF_SSID: usize = 12;
const OFF_PASSWORD: usize = OFF_SSID + WIFI_SSID_MAX;
const OFF_CRC: usize = OFF_PASSWORD + WIFI_PASSWORD_MAX;
const OFF_PADDING: usize = OFF_CRC + 4;
const RESERVED_FILL: u8 = 0xFF;
const ERASED_BYTE: u8 = 0xFF;

/// Minimal flash primitive surface. `false` signals a physical failure.
pub trait CredentialFlash {
    /// Read `buf.len()` bytes at `offset` into `buf`.
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> bool;
    /// Erase `len` bytes starting at `offset` (bytes read back as `0xFF`).
    fn erase(&mut self, offset: u32, len: u32) -> bool;
    /// Write `data` at `offset`.
    fn write(&mut self, offset: u32, data: &[u8]) -> bool;
}

/// Failure modes. Credential contents are never logged or exposed here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashStoreError {
    Read,
    Erase,
    Write,
    Verify,
    InvalidCredentials,
}

fn valid_credentials(creds: &WifiCredentials) -> bool {
    let ssid_len = creds.ssid_len as usize;
    let password_len = creds.password_len as usize;
    if ssid_len == 0 || ssid_len > WIFI_SSID_MAX {
        return false;
    }
    if password_len > WIFI_PASSWORD_MAX {
        return false;
    }
    true
}

fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn encode_record(creds: &WifiCredentials, generation: u32, out: &mut [u8; RECORD_LEN]) {
    for b in out.iter_mut() {
        *b = ERASED_BYTE;
    }
    out[0..4].copy_from_slice(&MAGIC.to_le_bytes());
    out[OFF_VERSION] = VERSION;
    out[OFF_SSID_LEN] = creds.ssid_len;
    out[OFF_PASSWORD_LEN] = creds.password_len;
    out[OFF_RESERVED] = RESERVED_FILL;
    out[OFF_GENERATION..OFF_GENERATION + 4].copy_from_slice(&generation.to_le_bytes());
    out[OFF_SSID..OFF_SSID + WIFI_SSID_MAX].copy_from_slice(&creds.ssid);
    out[OFF_PASSWORD..OFF_PASSWORD + WIFI_PASSWORD_MAX].copy_from_slice(&creds.password);
    let crc = crc32_ieee(&out[0..OFF_CRC]);
    out[OFF_CRC..OFF_CRC + 4].copy_from_slice(&crc.to_le_bytes());
    // OFF_PADDING..RECORD_LEN already 0xFF.
}

fn decode_record(buf: &[u8; RECORD_LEN]) -> Option<(WifiCredentials, u32)> {
    if OFF_PADDING != 112 || RECORD_LEN != 128 {
        return None;
    }
    let magic = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    if magic != MAGIC {
        return None;
    }
    if buf[OFF_VERSION] != VERSION {
        return None;
    }
    if buf[OFF_RESERVED] != RESERVED_FILL {
        return None;
    }
    let ssid_len = buf[OFF_SSID_LEN] as usize;
    let password_len = buf[OFF_PASSWORD_LEN] as usize;
    if ssid_len == 0 || ssid_len > WIFI_SSID_MAX {
        return None;
    }
    if password_len > WIFI_PASSWORD_MAX {
        return None;
    }
    let stored = u32::from_le_bytes([
        buf[OFF_CRC],
        buf[OFF_CRC + 1],
        buf[OFF_CRC + 2],
        buf[OFF_CRC + 3],
    ]);
    if crc32_ieee(&buf[0..OFF_CRC]) != stored {
        return None;
    }
    for b in &buf[OFF_PADDING..RECORD_LEN] {
        if *b != ERASED_BYTE {
            return None;
        }
    }
    let mut ssid = [0u8; WIFI_SSID_MAX];
    ssid.copy_from_slice(&buf[OFF_SSID..OFF_SSID + WIFI_SSID_MAX]);
    let mut password = [0u8; WIFI_PASSWORD_MAX];
    password.copy_from_slice(&buf[OFF_PASSWORD..OFF_PASSWORD + WIFI_PASSWORD_MAX]);
    let generation = u32::from_le_bytes([
        buf[OFF_GENERATION],
        buf[OFF_GENERATION + 1],
        buf[OFF_GENERATION + 2],
        buf[OFF_GENERATION + 3],
    ]);
    Some((
        WifiCredentials {
            ssid,
            ssid_len: ssid_len as u8,
            password,
            password_len: password_len as u8,
        },
        generation,
    ))
}

fn slot_offset(slot: usize) -> Option<u32> {
    if slot >= SLOT_COUNT {
        return None;
    }
    let offset = (slot as u32).checked_mul(SECTOR_SIZE)?;
    let end = offset.checked_add(RECORD_LEN as u32)?;
    if end > PARTITION_SIZE {
        return None;
    }
    let sector_end = offset.checked_add(SECTOR_SIZE)?;
    if sector_end > PARTITION_SIZE {
        return None;
    }
    Some(offset)
}

/// Wrap-safe "a is newer than b": half the u32 space ahead of b.
fn is_newer(a: u32, b: u32) -> bool {
    a != b && a.wrapping_sub(b) < 0x8000_0000
}

fn read_slot<F: CredentialFlash>(
    flash: &mut F,
    slot: usize,
) -> Result<Option<(WifiCredentials, u32)>, FlashStoreError> {
    let offset = slot_offset(slot).ok_or(FlashStoreError::Read)?;
    let mut buf = [0u8; RECORD_LEN];
    if !flash.read(offset, &mut buf) {
        return Err(FlashStoreError::Read);
    }
    Ok(decode_record(&buf))
}

/// Load the newest valid record. Erased/corrupt records count as absent;
/// only a failed physical read is [`FlashStoreError::Read`].
pub fn load<F: CredentialFlash>(flash: &mut F) -> Result<Option<WifiCredentials>, FlashStoreError> {
    let a = read_slot(flash, 0)?;
    let b = read_slot(flash, 1)?;
    match (a, b) {
        (None, None) => Ok(None),
        (Some((creds, _)), None) => Ok(Some(creds)),
        (None, Some((creds, _))) => Ok(Some(creds)),
        (Some((creds_a, gen_a)), Some((creds_b, gen_b))) => {
            if is_newer(gen_b, gen_a) {
                Ok(Some(creds_b))
            } else if is_newer(gen_a, gen_b) {
                Ok(Some(creds_a))
            } else {
                // Equal generations should not happen; stay deterministic.
                Ok(Some(creds_a))
            }
        }
    }
}

/// Store credentials into the inactive (or older) slot, then verify by
/// reading the record back. The untouched slot keeps the prior record, so a
/// torn erase/write on the target cannot destroy the previous generation.
pub fn store<F: CredentialFlash>(
    flash: &mut F,
    creds: &WifiCredentials,
) -> Result<(), FlashStoreError> {
    if !valid_credentials(creds) {
        return Err(FlashStoreError::InvalidCredentials);
    }
    let a = read_slot(flash, 0)?;
    let b = read_slot(flash, 1)?;
    let (target, generation) = match (a, b) {
        (None, None) => (0, 0u32),
        (Some((_, gen_a)), None) => (1, gen_a.wrapping_add(1)),
        (None, Some((_, gen_b))) => (0, gen_b.wrapping_add(1)),
        (Some((_, gen_a)), Some((_, gen_b))) => {
            if is_newer(gen_b, gen_a) {
                (0, gen_b.wrapping_add(1))
            } else if is_newer(gen_a, gen_b) {
                (1, gen_a.wrapping_add(1))
            } else {
                (1, gen_a.wrapping_add(1))
            }
        }
    };
    let offset = slot_offset(target).ok_or(FlashStoreError::Write)?;
    let mut record = [ERASED_BYTE; RECORD_LEN];
    encode_record(creds, generation, &mut record);
    if !flash.erase(offset, SECTOR_SIZE) {
        return Err(FlashStoreError::Erase);
    }
    if !flash.write(offset, &record) {
        return Err(FlashStoreError::Write);
    }
    let mut verify = [0u8; RECORD_LEN];
    if !flash.read(offset, &mut verify) {
        return Err(FlashStoreError::Verify);
    }
    if verify != record {
        return Err(FlashStoreError::Verify);
    }
    match decode_record(&verify) {
        Some((back, gen)) if gen == generation && back == *creds => Ok(()),
        _ => Err(FlashStoreError::Verify),
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{WifiCredentials, WIFI_PASSWORD_MAX, WIFI_SSID_MAX};
    use super::*;

    const FAKE_LEN: usize = 8192;

    struct FakeFlash {
        data: [u8; FAKE_LEN],
        fail_read: bool,
        fail_erase: bool,
        fail_write: bool,
        corrupt_verify: bool,
    }

    impl FakeFlash {
        fn erased() -> Self {
            Self {
                data: [ERASED_BYTE; FAKE_LEN],
                fail_read: false,
                fail_erase: false,
                fail_write: false,
                corrupt_verify: false,
            }
        }

        fn slot_bytes(&self, slot: usize) -> &[u8] {
            let off = slot * SECTOR_SIZE as usize;
            &self.data[off..off + RECORD_LEN]
        }

        fn generation_of(&self, slot: usize) -> Option<u32> {
            let mut rec = [0u8; RECORD_LEN];
            rec.copy_from_slice(self.slot_bytes(slot));
            decode_record(&rec).map(|(_, gen)| gen)
        }
    }

    impl CredentialFlash for FakeFlash {
        fn read(&mut self, offset: u32, buf: &mut [u8]) -> bool {
            if self.fail_read {
                return false;
            }
            let off = offset as usize;
            let end = match off.checked_add(buf.len()) {
                Some(end) => end,
                None => return false,
            };
            if end > self.data.len() {
                return false;
            }
            buf.copy_from_slice(&self.data[off..end]);
            true
        }

        fn erase(&mut self, offset: u32, len: u32) -> bool {
            if self.fail_erase {
                return false;
            }
            let off = offset as usize;
            let len = len as usize;
            let end = match off.checked_add(len) {
                Some(end) => end,
                None => return false,
            };
            if end > self.data.len() {
                return false;
            }
            for b in &mut self.data[off..end] {
                *b = ERASED_BYTE;
            }
            true
        }

        fn write(&mut self, offset: u32, data: &[u8]) -> bool {
            if self.fail_write {
                return false;
            }
            let off = offset as usize;
            let end = match off.checked_add(data.len()) {
                Some(end) => end,
                None => return false,
            };
            if end > self.data.len() {
                return false;
            }
            // Flash-like semantics: bits only transition 1 -> 0.
            for (i, &b) in data.iter().enumerate() {
                self.data[off + i] &= b;
            }
            if self.corrupt_verify {
                self.data[off] ^= 0x01;
            }
            true
        }
    }

    fn creds(ssid: &[u8], password: &[u8]) -> WifiCredentials {
        let mut out = WifiCredentials {
            ssid: [0u8; WIFI_SSID_MAX],
            ssid_len: ssid.len() as u8,
            password: [0u8; WIFI_PASSWORD_MAX],
            password_len: password.len() as u8,
        };
        out.ssid[..ssid.len()].copy_from_slice(ssid);
        out.password[..password.len()].copy_from_slice(password);
        out
    }

    fn write_raw_record(flash: &mut FakeFlash, slot: usize, c: &WifiCredentials, gen: u32) {
        let mut rec = [ERASED_BYTE; RECORD_LEN];
        encode_record(c, gen, &mut rec);
        let off = slot * SECTOR_SIZE as usize;
        // Bypass AND-semantics for test setup: erase then install bytes directly.
        for b in &mut flash.data[off..off + SECTOR_SIZE as usize] {
            *b = ERASED_BYTE;
        }
        flash.data[off..off + RECORD_LEN].copy_from_slice(&rec);
    }

    #[test]
    fn erased_flash_loads_absent() {
        let mut flash = FakeFlash::erased();
        assert_eq!(load(&mut flash), Ok(None));
    }

    #[test]
    fn roundtrip_single_store() {
        let mut flash = FakeFlash::erased();
        let c = creds(b"home-ap", b"secret-pw");
        assert_eq!(store(&mut flash, &c), Ok(()));
        assert_eq!(load(&mut flash), Ok(Some(c)));
    }

    #[test]
    fn empty_password_roundtrips() {
        let mut flash = FakeFlash::erased();
        let c = creds(b"open-net", b"");
        assert_eq!(store(&mut flash, &c), Ok(()));
        assert_eq!(load(&mut flash), Ok(Some(c)));
    }

    #[test]
    fn successive_stores_alternate_slots() {
        let mut flash = FakeFlash::erased();
        let a = creds(b"net-a", b"pw-a");
        let b = creds(b"net-b", b"pw-b");
        assert_eq!(store(&mut flash, &a), Ok(()));
        assert_eq!(flash.generation_of(0), Some(0));
        assert_eq!(load(&mut flash), Ok(Some(a)));
        assert_eq!(store(&mut flash, &b), Ok(()));
        // Second store must target the other sector with the next generation.
        assert_eq!(flash.generation_of(1), Some(1));
        assert_eq!(load(&mut flash), Ok(Some(b)));
        // Third store wraps back to slot 0 with generation 2.
        let c = creds(b"net-c", b"pw-c");
        assert_eq!(store(&mut flash, &c), Ok(()));
        assert_eq!(flash.generation_of(0), Some(2));
        assert_eq!(load(&mut flash), Ok(Some(c)));
    }

    #[test]
    fn corruption_of_newest_falls_back_to_older() {
        let mut flash = FakeFlash::erased();
        let a = creds(b"net-a", b"pw-a");
        let b = creds(b"net-b", b"pw-b");
        assert_eq!(store(&mut flash, &a), Ok(()));
        assert_eq!(store(&mut flash, &b), Ok(()));
        // Newest (slot 1, gen 1) is corrupted; loader must fall back to slot 0.
        let off = SECTOR_SIZE as usize;
        flash.data[off + OFF_SSID] ^= 0xFF;
        assert_eq!(load(&mut flash), Ok(Some(a)));
    }

    #[test]
    fn corrupt_magic_counts_as_absent() {
        let mut flash = FakeFlash::erased();
        let a = creds(b"net-a", b"pw-a");
        assert_eq!(store(&mut flash, &a), Ok(()));
        let mut raw = [0u8; RECORD_LEN];
        raw.copy_from_slice(flash.slot_bytes(0));
        raw[0] ^= 0xFF;
        flash.data[0..RECORD_LEN].copy_from_slice(&raw);
        assert_eq!(load(&mut flash), Ok(None));
    }

    #[test]
    fn torn_update_to_inactive_slot_preserves_prior() {
        let mut flash = FakeFlash::erased();
        let a = creds(b"net-a", b"pw-a");
        assert_eq!(store(&mut flash, &a), Ok(()));
        // Simulate a torn second update: inactive sector erased, then only a
        // partial/corrupt record written. Prior record in slot 0 is untouched.
        for b in &mut flash.data[SECTOR_SIZE as usize..2 * SECTOR_SIZE as usize] {
            *b = ERASED_BYTE;
        }
        flash.data[SECTOR_SIZE as usize] = 0x00; // garbage magic fragment
        assert_eq!(load(&mut flash), Ok(Some(a)));
    }

    #[test]
    fn failed_store_leaves_prior_intact() {
        let mut flash = FakeFlash::erased();
        let a = creds(b"net-a", b"pw-a");
        let b = creds(b"net-b", b"pw-b");
        assert_eq!(store(&mut flash, &a), Ok(()));
        flash.fail_erase = true;
        assert_eq!(store(&mut flash, &b), Err(FlashStoreError::Erase));
        flash.fail_erase = false;
        assert_eq!(load(&mut flash), Ok(Some(a)));

        flash.fail_write = true;
        assert_eq!(store(&mut flash, &b), Err(FlashStoreError::Write));
        flash.fail_write = false;
        // Erase of the target succeeded before the failed write, so the target
        // slot is blank and the prior slot still serves reads.
        assert_eq!(load(&mut flash), Ok(Some(a)));
    }

    #[test]
    fn wrap_around_generation_selects_newest() {
        let mut flash = FakeFlash::erased();
        let old = creds(b"old-net", b"old-pw");
        let new = creds(b"new-net", b"new-pw");
        write_raw_record(&mut flash, 0, &old, u32::MAX);
        write_raw_record(&mut flash, 1, &new, 0);
        assert_eq!(load(&mut flash), Ok(Some(new)));

        // Storing again must advance past the wrapped generation.
        let next = creds(b"next-net", b"next-pw");
        assert_eq!(store(&mut flash, &next), Ok(()));
        assert_eq!(load(&mut flash), Ok(Some(next)));
        // The older slot (gen MAX) was the target, now holding gen 1.
        assert_eq!(flash.generation_of(0), Some(1));
    }

    #[test]
    fn store_rejects_invalid_lengths() {
        let mut flash = FakeFlash::erased();
        let mut empty_ssid = creds(b"x", b"pw");
        empty_ssid.ssid_len = 0;
        assert_eq!(
            store(&mut flash, &empty_ssid),
            Err(FlashStoreError::InvalidCredentials)
        );
        let mut long_ssid = creds(b"x", b"pw");
        long_ssid.ssid_len = (WIFI_SSID_MAX + 1) as u8;
        assert_eq!(
            store(&mut flash, &long_ssid),
            Err(FlashStoreError::InvalidCredentials)
        );
        let mut long_pw = creds(b"x", b"pw");
        long_pw.password_len = (WIFI_PASSWORD_MAX + 1) as u8;
        assert_eq!(
            store(&mut flash, &long_pw),
            Err(FlashStoreError::InvalidCredentials)
        );
        // Rejected stores must not disturb the partition.
        assert_eq!(load(&mut flash), Ok(None));
    }

    #[test]
    fn record_with_invalid_lengths_loads_absent() {
        let mut flash = FakeFlash::erased();
        let c = creds(b"net-a", b"pw-a");
        let mut rec = [ERASED_BYTE; RECORD_LEN];
        encode_record(&c, 7, &mut rec);
        // Corrupt the length field, then repair the CRC so only the length
        // itself is at fault; decode must still reject the record.
        rec[OFF_SSID_LEN] = (WIFI_SSID_MAX + 1) as u8;
        let crc = crc32_ieee(&rec[0..OFF_CRC]);
        rec[OFF_CRC..OFF_CRC + 4].copy_from_slice(&crc.to_le_bytes());
        flash.data[0..RECORD_LEN].copy_from_slice(&rec);
        assert_eq!(load(&mut flash), Ok(None));
    }

    #[test]
    fn bad_padding_loads_absent() {
        let mut flash = FakeFlash::erased();
        let c = creds(b"net-a", b"pw-a");
        assert_eq!(store(&mut flash, &c), Ok(()));
        flash.data[OFF_PADDING] = 0x00;
        assert_eq!(load(&mut flash), Ok(None));
    }

    #[test]
    fn physical_read_failure_is_read_error() {
        let mut flash = FakeFlash::erased();
        flash.fail_read = true;
        assert_eq!(load(&mut flash), Err(FlashStoreError::Read));
        let c = creds(b"net-a", b"pw-a");
        assert_eq!(store(&mut flash, &c), Err(FlashStoreError::Read));
    }

    #[test]
    fn verify_failure_is_verify_error() {
        let mut flash = FakeFlash::erased();
        flash.corrupt_verify = true;
        let c = creds(b"net-a", b"pw-a");
        assert_eq!(store(&mut flash, &c), Err(FlashStoreError::Verify));
    }
}
