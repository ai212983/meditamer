//! Central-role pairing setup shared by every `platform/connectivity/ble` module that
//! builds a `trouble_host::Stack` for the central role (ADR-0016: "Add
//! pairing and bonding to the shared BLE central role"). Trouble 0.8 seeds
//! its security CSPRNG from the controller's `LE Rand` command during host
//! initialization. The bounded, host-testable bond *table* itself lives in
//! [`crate::bond`], kept deliberately free of `trouble-host`.

/// Convert a stored [`crate::bond::BondRecord`] into the
/// `trouble_host::BondInformation` `Stack::add_bond_information` expects,
/// so a previously paired peer's key material is registered before it
/// reconnects and does not have to pair again.
pub fn bond_information_from_record(
    record: &crate::bond::BondRecord,
) -> trouble_host::BondInformation {
    use trouble_host::prelude::*;
    let identity = Identity {
        addr: Address::new(
            AddrKind::new(record.peer_address_kind),
            BdAddr::new(record.peer_address),
        ),
        irk: record.irk.and_then(IdentityResolvingKey::from_le_bytes),
    };
    let security_level = match record.security_level {
        crate::bond::SecurityLevel::NoEncryption => SecurityLevel::NoEncryption,
        crate::bond::SecurityLevel::Encrypted => SecurityLevel::Encrypted,
        crate::bond::SecurityLevel::EncryptedAuthenticated => SecurityLevel::EncryptedAuthenticated,
    };
    let mut bond = BondInformation::new(
        identity,
        LongTermKey::new(u128::from_le_bytes(record.ltk)),
        security_level,
        record.is_bonded,
    );
    bond.ediv = record.ediv;
    bond.rand = record.rand;
    bond.encryption_key_len = record.encryption_key_len;
    bond
}

/// The inverse of [`bond_information_from_record`]: what
/// `ConnectionEvent::PairingComplete`'s `bond` field hands back after a
/// successful bonded pairing, converted to this crate's own chip-free,
/// storable [`crate::bond::BondRecord`].
pub fn record_from_bond_information(
    bond: &trouble_host::BondInformation,
) -> crate::bond::BondRecord {
    use trouble_host::prelude::*;
    let peer_address: [u8; 6] = bond.identity.addr.addr.raw().try_into().unwrap_or([0; 6]);
    let irk = bond.identity.irk.map(IdentityResolvingKey::to_le_bytes);
    let security_level = match bond.security_level {
        SecurityLevel::NoEncryption => crate::bond::SecurityLevel::NoEncryption,
        SecurityLevel::Encrypted => crate::bond::SecurityLevel::Encrypted,
        SecurityLevel::EncryptedAuthenticated => crate::bond::SecurityLevel::EncryptedAuthenticated,
    };
    crate::bond::BondRecord {
        peer_address_kind: bond.identity.addr.kind.as_raw(),
        peer_address,
        irk,
        ltk: bond.ltk.0.to_le_bytes(),
        security_level,
        is_bonded: bond.is_bonded,
        ediv: bond.ediv,
        rand: bond.rand,
        encryption_key_len: bond.encryption_key_len,
    }
}
