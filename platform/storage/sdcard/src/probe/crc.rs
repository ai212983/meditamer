//! CRC-16 used by SD SPI data blocks.
//!
//! SD data CRC is CRC-16-CCITT, polynomial `0x1021`, initial value zero,
//! transmitted most-significant byte first. This is the CRC-16-be primitive
//! documented by the ESP ROM CRC implementation and matches the SD wire order.
//! SD Physical Layer Simplified Specification v6.00, sections 7.2.3/7.2.6:
//! read data and CSD blocks carry CRC16 even with command CRC checking off.
//! No CMD59 or write-CRC policy change is required for read validation.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mismatch {
    pub expected: u16,
    pub received: u16,
}

pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

pub fn validate(data: &[u8], received: u16) -> Result<(), Mismatch> {
    let expected = crc16(data);
    if received == expected {
        Ok(())
    } else {
        Err(Mismatch { expected, received })
    }
}

/// Publish a verified read to the caller and cache atomically with respect to
/// validation: a bad payload cannot update either output or cache validity.
pub fn validate_and_publish(
    data: &[u8],
    received: u16,
    out: &mut [u8],
    cached_lba: &mut Option<u32>,
    lba: u32,
) -> Result<(), Mismatch> {
    debug_assert_eq!(data.len(), out.len());
    *cached_lba = None;
    validate(data, received)?;
    out.copy_from_slice(data);
    *cached_lba = Some(lba);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{crc16, validate, validate_and_publish, Mismatch};

    #[test]
    fn independent_crc16_vectors() {
        assert_eq!(crc16(b""), 0x0000);
        assert_eq!(crc16(b"123456789"), 0x31C3);
        assert_eq!(crc16(&[0x00]), 0x0000);
        assert_eq!(crc16(&[0xFF]), 0x1EF0);
    }

    #[test]
    fn wire_order_is_most_significant_byte_first() {
        let crc = crc16(b"123456789");
        assert_eq!(crc.to_be_bytes(), [0x31, 0xC3]);
        assert_ne!(crc.to_be_bytes(), crc.to_le_bytes());
    }

    #[test]
    fn corruption_changes_crc() {
        let mut data = *b"123456789";
        let expected = crc16(&data);
        data[4] ^= 0x01;
        assert_ne!(crc16(&data), expected);
    }

    #[test]
    fn valid_csd_and_sector_lengths_validate() {
        let csd = [0xA5; 16];
        let sector = [0x3C; 512];
        assert_eq!(validate(&csd, crc16(&csd)), Ok(()));
        assert_eq!(validate(&sector, crc16(&sector)), Ok(()));
    }

    #[test]
    fn payload_or_crc_byte_corruption_is_rejected() {
        let mut payload = [0u8; 512];
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte = index as u8;
        }
        let crc = crc16(&payload);
        payload[211] ^= 0x80;
        assert_eq!(
            validate(&payload, crc),
            Err(Mismatch {
                expected: crc16(&payload),
                received: crc
            })
        );
        assert_eq!(
            validate(&payload, crc ^ 0x0100),
            Err(Mismatch {
                expected: crc16(&payload),
                received: crc ^ 0x0100
            })
        );
    }

    #[test]
    fn failed_publish_leaves_output_unchanged_and_allows_valid_reread() {
        let data = [0x5A; 512];
        let mut out = [0xC3; 512];
        let mut cached_lba = Some(7);
        let bad = crc16(&data) ^ 1;
        assert!(validate_and_publish(&data, bad, &mut out, &mut cached_lba, 9).is_err());
        assert_eq!(out, [0xC3; 512]);
        assert_eq!(cached_lba, None);

        cached_lba = None;
        assert_eq!(
            validate_and_publish(&data, crc16(&data), &mut out, &mut cached_lba, 9),
            Ok(())
        );
        assert_eq!(out, data);
        assert_eq!(cached_lba, Some(9));
    }
}
