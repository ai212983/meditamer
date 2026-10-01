#![no_std]
//! Host-testable ESP-IDF `otadata` record CRC.
//!
//! Extracted from `src/firmware/update.rs` (ADR-0014 Phase 5) so the exact
//! implementation the device and the factory updater both use is
//! host-testable: the `meditamer` firmware crate itself is `no_std`/Xtensa
//! and `[lib] test = false`, so an inline `#[cfg(test)]` module there is
//! never actually exercised by any host-test path. `sequence`'s CRC is what
//! `esp_bootloader_esp_idf::ota::Ota`'s otadata records store to validate a
//! record's `ota_seq` field; get this wrong and slot selection silently
//! resolves to the wrong (or a corrupt) app partition.
//!
//! `tools/hostctl/src/workflows/single_production/otadata.rs` maintains its
//! own independent implementation of the same CRC (via the `crc` crate's
//! `Algorithm` builder, cross-checked against real hardware) rather than
//! depending on this crate — consolidating the two is a follow-up, not part
//! of this extraction.

/// `esp_rom_crc32_le(0xFFFFFFFF, &sequence.to_le_bytes(), 4)`, the exact
/// algorithm ESP-IDF's bootloader uses for otadata's sequence-number CRC —
/// not the textbook/zlib CRC-32 (`CRC_32_ISO_HDLC`, `init = 0`).
pub fn ota_crc(sequence: u32) -> u32 {
    let mut crc = 0u32;
    for byte in sequence.to_le_bytes() {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ota_crc_matches_esp_idf_examples() {
        assert_eq!(ota_crc(1), 0x4743_989a);
        assert_eq!(ota_crc(2), 0x55f6_3774);
        assert_eq!(ota_crc(3), 0xed4a_5011);
    }
}
