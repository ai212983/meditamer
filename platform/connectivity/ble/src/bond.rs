//! Bonded-peer storage for the shared BLE central role (ADR-0016: "Add
//! pairing and bonding to the shared BLE central role").
//!
//! Owns a bounded, fixed-capacity table of [`BondRecord`]s and each
//! record's fixed byte layout. Host-testable and clock-free like the rest
//! of this crate, and deliberately `trouble-host`-free: [`live_connect`]
//! (chip-only) is the boundary that converts a real
//! `trouble_host::BondInformation` to/from [`BondRecord`] and drives this
//! table from real pairing events. Raw non-volatile read/write of the
//! whole table is a small port each `targets/*` crate implements (ADR-0016's
//! ownership split, matching how [`crate::gatt`] owns connection *logic*
//! while a target owns the radio) -- this module only owns the table and
//! its wire format, never a flash chip.
//!
//! Inserting a new peer past [`crate::capacity::BOND_TABLE_MAX`] is a
//! rejected [`BondTableError::Full`], never a silent eviction of a working
//! bond (ADR-0016's own reasoning for why four slots is enough that this
//! should stay rare) -- callers that want to make room call [`BondTable::remove`]
//! explicitly first.

use crate::capacity::{BOND_RECORD_BYTES, BOND_TABLE_BYTES, BOND_TABLE_MAX};

/// A fixed-size wire value did not decode: wrong length, an unrecognized
/// format version, or a byte that does not name a legal enumerated value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    WrongLength,
    UnknownFormatVersion,
    UnknownSecurityLevel,
}

/// [`BondTable`] operations that can fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BondTableError {
    /// The table already holds [`crate::capacity::BOND_TABLE_MAX`] distinct
    /// peers and `peer_address` names one not already present.
    Full,
    /// No record matches the given `peer_address`.
    NotFound,
}

/// Bond security level, mirroring `trouble_host::connection::SecurityLevel`
/// (`NoEncryption`/`Encrypted`/`EncryptedAuthenticated`) without depending
/// on `trouble-host` -- this module stays host-testable and chip-free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityLevel {
    NoEncryption,
    Encrypted,
    EncryptedAuthenticated,
}

impl SecurityLevel {
    fn to_byte(self) -> u8 {
        match self {
            Self::NoEncryption => 0,
            Self::Encrypted => 1,
            Self::EncryptedAuthenticated => 2,
        }
    }

    fn from_byte(byte: u8) -> Result<Self, DecodeError> {
        match byte {
            0 => Ok(Self::NoEncryption),
            1 => Ok(Self::Encrypted),
            2 => Ok(Self::EncryptedAuthenticated),
            _ => Err(DecodeError::UnknownSecurityLevel),
        }
    }
}

const FORMAT_VERSION: u8 = 2;
const LEGACY_FORMAT_VERSION: u8 = 1;
const LEGACY_BOND_RECORD_BYTES: usize = 48;
const LEGACY_BOND_TABLE_BYTES: usize = BOND_TABLE_MAX * LEGACY_BOND_RECORD_BYTES;

/// One bonded peer's identity and key material, exactly the fields
/// `trouble_host::BondInformation`/`Identity` carry (an address, an
/// optional Identity Resolving Key, a Long Term Key, a security level, and
/// whether the pairing was actually bonded), kept in this crate's own
/// chip-free types so [`BondTable`] never needs `trouble-host` in scope.
///
/// Wire layout (64 bytes, all reserved bytes zero):
///
/// | Offset | Bytes | Field |
/// | --- | --- | --- |
/// | 0 | 1 | `format_version` |
/// | 1 | 1 | `peer_address_kind` |
/// | 2 | 6 | `peer_address` |
/// | 8 | 1 | `irk` present flag |
/// | 9 | 16 | `irk` (zero if absent) |
/// | 25 | 16 | `ltk` |
/// | 41 | 1 | `security_level` |
/// | 42 | 1 | `is_bonded` flag |
/// | 43 | 2 | legacy `ediv` |
/// | 45 | 8 | legacy `rand` |
/// | 53 | 1 | negotiated `encryption_key_len` |
/// | 54 | 10 | reserved |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BondRecord {
    pub peer_address_kind: u8,
    pub peer_address: [u8; 6],
    pub irk: Option<[u8; 16]>,
    pub ltk: [u8; 16],
    pub security_level: SecurityLevel,
    pub is_bonded: bool,
    pub ediv: u16,
    pub rand: [u8; 8],
    pub encryption_key_len: u8,
}

impl BondRecord {
    pub fn encode(&self) -> [u8; BOND_RECORD_BYTES] {
        let mut out = [0u8; BOND_RECORD_BYTES];
        out[0] = FORMAT_VERSION;
        out[1] = self.peer_address_kind;
        out[2..8].copy_from_slice(&self.peer_address);
        if let Some(irk) = self.irk {
            out[8] = 1;
            out[9..25].copy_from_slice(&irk);
        }
        out[25..41].copy_from_slice(&self.ltk);
        out[41] = self.security_level.to_byte();
        out[42] = u8::from(self.is_bonded);
        out[43..45].copy_from_slice(&self.ediv.to_le_bytes());
        out[45..53].copy_from_slice(&self.rand);
        out[53] = self.encryption_key_len;
        // out[54..64] stays reserved/zero.
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        match bytes.len() {
            BOND_RECORD_BYTES if bytes[0] == FORMAT_VERSION => Self::decode_v2(bytes),
            LEGACY_BOND_RECORD_BYTES if bytes[0] == LEGACY_FORMAT_VERSION => Self::decode_v1(bytes),
            BOND_RECORD_BYTES | LEGACY_BOND_RECORD_BYTES => Err(DecodeError::UnknownFormatVersion),
            _ => Err(DecodeError::WrongLength),
        }
    }

    fn decode_v2(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut peer_address = [0u8; 6];
        peer_address.copy_from_slice(&bytes[2..8]);
        let irk = if bytes[8] == 1 {
            let mut irk = [0u8; 16];
            irk.copy_from_slice(&bytes[9..25]);
            Some(irk)
        } else {
            None
        };
        let mut ltk = [0u8; 16];
        ltk.copy_from_slice(&bytes[25..41]);
        let mut rand = [0u8; 8];
        rand.copy_from_slice(&bytes[45..53]);
        Ok(Self {
            peer_address_kind: bytes[1],
            peer_address,
            irk,
            ltk,
            security_level: SecurityLevel::from_byte(bytes[41])?,
            is_bonded: bytes[42] != 0,
            ediv: u16::from_le_bytes([bytes[43], bytes[44]]),
            rand,
            encryption_key_len: bytes[53],
        })
    }

    fn decode_v1(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut peer_address = [0u8; 6];
        peer_address.copy_from_slice(&bytes[1..7]);
        let irk = if bytes[7] == 1 {
            let mut irk = [0u8; 16];
            irk.copy_from_slice(&bytes[8..24]);
            Some(irk)
        } else {
            None
        };
        let mut ltk = [0u8; 16];
        ltk.copy_from_slice(&bytes[24..40]);
        Ok(Self {
            // V1 stored no kind or legacy-pairing diversifiers; its records
            // came from the Secure Connections-only Trouble 0.6 stack.
            peer_address_kind: 0,
            peer_address,
            irk,
            ltk,
            security_level: SecurityLevel::from_byte(bytes[40])?,
            is_bonded: bytes[41] != 0,
            ediv: 0,
            rand: [0; 8],
            encryption_key_len: 16,
        })
    }

    /// True for an all-zero slot ([`BondTable::decode`]'s "unused" marker
    /// -- `format_version` `0` never decodes, so a genuine record can never
    /// be mistaken for an empty one).
    fn is_unused_slot(bytes: &[u8]) -> bool {
        bytes.iter().all(|&byte| byte == 0)
    }
}

/// Bounded set of [`BondRecord`]s (ADR-0016, [`crate::capacity::BOND_TABLE_MAX`]
/// slots).
#[derive(Clone, Debug)]
pub struct BondTable {
    records: heapless::Vec<BondRecord, BOND_TABLE_MAX>,
}

impl Default for BondTable {
    fn default() -> Self {
        Self::new()
    }
}

impl BondTable {
    pub fn new() -> Self {
        Self {
            records: heapless::Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &BondRecord> {
        self.records.iter()
    }

    pub fn find(&self, peer_address: [u8; 6]) -> Option<&BondRecord> {
        self.records
            .iter()
            .find(|record| record.peer_address == peer_address)
    }

    /// Insert a new peer, or replace an existing peer's record (matched by
    /// `peer_address`) in place. Replacing never counts against capacity;
    /// a genuinely new peer once the table already holds
    /// [`crate::capacity::BOND_TABLE_MAX`] distinct peers is rejected
    /// rather than silently evicting a working bond.
    pub fn insert(&mut self, record: BondRecord) -> Result<(), BondTableError> {
        if let Some(slot) = self
            .records
            .iter_mut()
            .find(|existing| existing.peer_address == record.peer_address)
        {
            *slot = record;
            return Ok(());
        }
        self.records.push(record).map_err(|_| BondTableError::Full)
    }

    /// Remove and return the record for `peer_address`, if present.
    pub fn remove(&mut self, peer_address: [u8; 6]) -> Result<BondRecord, BondTableError> {
        let index = self
            .records
            .iter()
            .position(|record| record.peer_address == peer_address)
            .ok_or(BondTableError::NotFound)?;
        Ok(self.records.swap_remove(index))
    }

    /// Encode the whole table for storage: [`crate::capacity::BOND_TABLE_MAX`]
    /// fixed-size slots, each a real record or an all-zero unused slot.
    pub fn encode(&self) -> [u8; BOND_TABLE_BYTES] {
        let mut out = [0u8; BOND_TABLE_BYTES];
        for (index, record) in self.records.iter().enumerate() {
            let start = index * BOND_RECORD_BYTES;
            out[start..start + BOND_RECORD_BYTES].copy_from_slice(&record.encode());
        }
        out
    }

    /// Decode a whole table from storage. Fails closed: any slot that is
    /// neither a valid record nor an all-zero unused slot aborts the whole
    /// restore (`Err`) rather than silently dropping just that one peer,
    /// since there is no way to tell a genuinely corrupt record from one
    /// that merely looks that way from a slot the caller should still
    /// trust.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let record_bytes = match bytes.len() {
            BOND_TABLE_BYTES => BOND_RECORD_BYTES,
            LEGACY_BOND_TABLE_BYTES => LEGACY_BOND_RECORD_BYTES,
            _ => return Err(DecodeError::WrongLength),
        };
        let mut table = Self::new();
        for chunk in bytes.chunks_exact(record_bytes) {
            if BondRecord::is_unused_slot(chunk) {
                continue;
            }
            let record = BondRecord::decode(chunk)?;
            // Either supported whole-table size yields exactly
            // `BOND_TABLE_MAX` chunks, so this never exceeds capacity.
            let _ = table.records.push(record);
        }
        Ok(table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record(peer_address: [u8; 6], irk: Option<[u8; 16]>) -> BondRecord {
        BondRecord {
            peer_address_kind: 1,
            peer_address,
            irk,
            ltk: [0x42; 16],
            security_level: SecurityLevel::EncryptedAuthenticated,
            is_bonded: true,
            ediv: 0x1234,
            rand: [0x56; 8],
            encryption_key_len: 16,
        }
    }

    #[test]
    fn record_round_trips_with_irk() {
        let record = sample_record([1, 2, 3, 4, 5, 6], Some([0x11; 16]));
        let encoded = record.encode();
        assert_eq!(encoded.len(), BOND_RECORD_BYTES);
        assert_eq!(BondRecord::decode(&encoded), Ok(record));
    }

    #[test]
    fn record_round_trips_without_irk() {
        let record = sample_record([9, 9, 9, 9, 9, 9], None);
        let encoded = record.encode();
        assert_eq!(BondRecord::decode(&encoded), Ok(record));
    }

    #[test]
    fn record_decode_rejects_wrong_length() {
        let short = [0u8; BOND_RECORD_BYTES - 1];
        assert_eq!(BondRecord::decode(&short), Err(DecodeError::WrongLength));
    }

    #[test]
    fn record_decode_rejects_unknown_format_version() {
        let mut bytes = sample_record([1; 6], None).encode();
        bytes[0] = 0xff;
        assert_eq!(
            BondRecord::decode(&bytes),
            Err(DecodeError::UnknownFormatVersion)
        );
    }

    #[test]
    fn record_decode_rejects_unknown_security_level() {
        let mut bytes = sample_record([1; 6], None).encode();
        bytes[41] = 0xff;
        assert_eq!(
            BondRecord::decode(&bytes),
            Err(DecodeError::UnknownSecurityLevel)
        );
    }

    #[test]
    fn all_zero_slot_never_decodes_as_a_record() {
        // format_version 0 is never legal, so an unused slot can never be
        // mistaken for a real record if decoded directly.
        assert_eq!(
            BondRecord::decode(&[0u8; BOND_RECORD_BYTES]),
            Err(DecodeError::UnknownFormatVersion)
        );
    }

    #[test]
    fn table_insert_find_remove() {
        let mut table = BondTable::new();
        let record = sample_record([1; 6], None);
        table.insert(record).unwrap();
        assert_eq!(table.len(), 1);
        assert_eq!(table.find([1; 6]), Some(&record));
        assert_eq!(table.remove([1; 6]), Ok(record));
        assert!(table.is_empty());
        assert_eq!(table.remove([1; 6]), Err(BondTableError::NotFound));
    }

    #[test]
    fn table_insert_same_peer_replaces_in_place() {
        let mut table = BondTable::new();
        table.insert(sample_record([1; 6], None)).unwrap();
        let replacement = sample_record([1; 6], Some([0x22; 16]));
        table.insert(replacement).unwrap();
        assert_eq!(table.len(), 1);
        assert_eq!(table.find([1; 6]), Some(&replacement));
    }

    #[test]
    fn table_rejects_a_new_peer_once_full_without_evicting() {
        let mut table = BondTable::new();
        for index in 0..BOND_TABLE_MAX {
            table.insert(sample_record([index as u8; 6], None)).unwrap();
        }
        let overflow_peer = [0xaa; 6];
        assert_eq!(
            table.insert(sample_record(overflow_peer, None)),
            Err(BondTableError::Full)
        );
        // Every original record is still present -- no silent eviction.
        for index in 0..BOND_TABLE_MAX {
            assert!(table.find([index as u8; 6]).is_some());
        }
        assert!(table.find(overflow_peer).is_none());
    }

    #[test]
    fn table_round_trips_a_partial_set() {
        let mut table = BondTable::new();
        table
            .insert(sample_record([1; 6], Some([0x33; 16])))
            .unwrap();
        table.insert(sample_record([2; 6], None)).unwrap();
        let encoded = table.encode();
        assert_eq!(encoded.len(), BOND_TABLE_BYTES);
        let decoded = BondTable::decode(&encoded).unwrap();
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded.find([1; 6]), table.find([1; 6]));
        assert_eq!(decoded.find([2; 6]), table.find([2; 6]));
    }

    #[test]
    fn legacy_v1_record_and_table_decode_with_secure_connections_defaults() {
        let source = sample_record([1, 2, 3, 4, 5, 6], Some([0x33; 16]));
        let mut legacy = [0u8; LEGACY_BOND_RECORD_BYTES];
        legacy[0] = LEGACY_FORMAT_VERSION;
        legacy[1..7].copy_from_slice(&source.peer_address);
        legacy[7] = 1;
        legacy[8..24].copy_from_slice(&source.irk.unwrap());
        legacy[24..40].copy_from_slice(&source.ltk);
        legacy[40] = source.security_level.to_byte();
        legacy[41] = u8::from(source.is_bonded);

        let decoded = BondRecord::decode(&legacy).unwrap();
        assert_eq!(decoded.peer_address_kind, 0);
        assert_eq!(decoded.peer_address, source.peer_address);
        assert_eq!(decoded.irk, source.irk);
        assert_eq!(decoded.ltk, source.ltk);
        assert_eq!(decoded.ediv, 0);
        assert_eq!(decoded.rand, [0; 8]);
        assert_eq!(decoded.encryption_key_len, 16);

        let mut legacy_table = [0u8; LEGACY_BOND_TABLE_BYTES];
        legacy_table[..LEGACY_BOND_RECORD_BYTES].copy_from_slice(&legacy);
        let table = BondTable::decode(&legacy_table).unwrap();
        assert_eq!(table.find(source.peer_address), Some(&decoded));
    }

    #[test]
    fn table_decode_rejects_wrong_length() {
        let short = [0u8; BOND_TABLE_BYTES - 1];
        assert_eq!(
            BondTable::decode(&short).unwrap_err(),
            DecodeError::WrongLength
        );
    }

    #[test]
    fn table_decode_fails_closed_on_a_corrupt_slot() {
        let mut bytes = [0u8; BOND_TABLE_BYTES];
        let valid = sample_record([1; 6], None).encode();
        bytes[..BOND_RECORD_BYTES].copy_from_slice(&valid);
        // Second slot: nonzero but not a legal record (bad format version).
        bytes[BOND_RECORD_BYTES] = 0xff;
        assert_eq!(
            BondTable::decode(&bytes).unwrap_err(),
            DecodeError::UnknownFormatVersion
        );
    }

    #[test]
    fn empty_table_encodes_as_all_zero() {
        assert_eq!(BondTable::new().encode(), [0u8; BOND_TABLE_BYTES]);
    }
}
