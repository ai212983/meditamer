//! Allocation-free scratch authorization and sector patterns for the SD probe.

/// Explicit qualification mode; invalid names never silently skip a test.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fault {
    None,
    MissingCompletion,
    WriteBusy,
    ReadCrc,
}

impl Fault {
    pub fn parse(value: Option<&str>, config: Config) -> Result<Self, &'static str> {
        match value {
            None => Ok(Self::None),
            Some("missing-completion") => Ok(Self::MissingCompletion),
            Some("write-busy") if config.scratch.is_some() => Ok(Self::WriteBusy),
            Some("write-busy") => Err("write_busy_requires_scratch"),
            Some("read-crc") => Ok(Self::ReadCrc),
            Some(_) => Err("invalid_fault"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    /// No region means that the probe must remain read-only.
    pub scratch: Option<ScratchRegion>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScratchRegion {
    pub start: u32,
    pub sectors: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigError {
    IncompleteScratchRegion,
    InvalidStart,
    InvalidSectorCount,
    BootSectorExcluded,
    TooFewSectors,
    RegionOverflow,
    BeyondCapacity,
}

impl Config {
    /// Parse the two build-time scratch settings. Both must be absent or valid;
    /// malformed authorization must never silently turn into a write request.
    pub fn parse(start: Option<&str>, sectors: Option<&str>) -> Result<Self, ConfigError> {
        let scratch = match (start, sectors) {
            (None, None) => None,
            (Some(start), Some(sectors)) => {
                let region = ScratchRegion {
                    start: decimal(start).ok_or(ConfigError::InvalidStart)?,
                    sectors: decimal(sectors).ok_or(ConfigError::InvalidSectorCount)?,
                };
                region.checked_end()?;
                Some(region)
            }
            _ => return Err(ConfigError::IncompleteScratchRegion),
        };
        Ok(Self { scratch })
    }
}

impl ScratchRegion {
    fn checked_end(self) -> Result<u32, ConfigError> {
        if self.start == 0 {
            return Err(ConfigError::BootSectorExcluded);
        }
        if self.sectors < 2 {
            return Err(ConfigError::TooFewSectors);
        }
        self.start
            .checked_add(self.sectors)
            .ok_or(ConfigError::RegionOverflow)
    }

    /// Validate the entire designated region against discovered card capacity
    /// before any write. The probe only uses the first two sectors of it.
    pub fn validate_capacity(self, capacity_sectors: u64) -> Result<Self, ConfigError> {
        if u64::from(self.checked_end()?) > capacity_sectors {
            return Err(ConfigError::BeyondCapacity);
        }
        Ok(self)
    }
}

fn decimal(value: &str) -> Option<u32> {
    if value.is_empty() {
        return None;
    }
    value.bytes().try_fold(0_u32, |number, digit| {
        if !digit.is_ascii_digit() {
            return None;
        }
        number.checked_mul(10)?.checked_add(u32::from(digit - b'0'))
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PatternMismatch {
    pub offset: usize,
    pub expected: u8,
    pub actual: u8,
}

fn pattern_byte(sector: u32, offset: usize) -> u8 {
    // Mix all address bytes into every four-byte group. Adjacent sectors cannot
    // have identical patterns, even when their low address byte wraps.
    let address = sector.to_le_bytes();
    address[offset % 4]
        .wrapping_add((offset as u8).wrapping_mul(37))
        .wrapping_add(((offset >> 8) as u8).wrapping_mul(113))
        ^ 0xa5
}

pub fn fill_pattern(buffer: &mut [u8; 512], sector: u32) {
    for (offset, byte) in buffer.iter_mut().enumerate() {
        *byte = pattern_byte(sector, offset);
    }
}

pub fn verify_pattern(buffer: &[u8; 512], sector: u32) -> Result<(), PatternMismatch> {
    for (offset, &actual) in buffer.iter().enumerate() {
        let expected = pattern_byte(sector, offset);
        if actual != expected {
            return Err(PatternMismatch {
                offset,
                expected,
                actual,
            });
        }
    }
    Ok(())
}

/// Choose a pattern that differs from a sector's freshly read contents, so a
/// skipped write cannot pass by retaining data from an earlier probe run.
pub fn fresh_seed(block: &[u8; 512], sector: u32) -> u32 {
    if verify_pattern(block, sector).is_ok() {
        // Complementing the seed changes every address byte. Addition and XOR
        // in pattern_byte are bijections, so the alternative pattern differs.
        sector ^ u32::MAX
    } else {
        sector
    }
}

/// Choose fresh, distinct patterns for a pair, catching stale or aliased data.
pub fn fresh_pair(first: &[u8; 512], second: &[u8; 512], start: u32) -> [u32; 2] {
    let first_seed = fresh_seed(first, start);
    let mut second_seed = start.wrapping_add(1);
    if second_seed == first_seed {
        second_seed = second_seed.wrapping_add(1);
    }
    if verify_pattern(second, second_seed).is_ok() {
        second_seed = second_seed.wrapping_add(1);
        if second_seed == first_seed {
            second_seed = second_seed.wrapping_add(1);
        }
    }
    // Seed-to-pattern mapping is injective. Only first_seed and (at most) the
    // second sector's existing pattern can be forbidden: three candidates suffice.
    [first_seed, second_seed]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_settings_are_read_only() {
        assert_eq!(Config::parse(None, None), Ok(Config { scratch: None }));
    }

    #[test]
    fn requires_complete_unsigned_decimal_settings() {
        for settings in [(Some("1"), None), (None, Some("2"))] {
            assert_eq!(
                Config::parse(settings.0, settings.1),
                Err(ConfigError::IncompleteScratchRegion)
            );
        }
        for invalid in [
            "",
            " ",
            " 1",
            "1 ",
            "+1",
            "-1",
            "0x10",
            "1_000",
            "4294967296",
        ] {
            assert_eq!(
                Config::parse(Some(invalid), Some("2")),
                Err(ConfigError::InvalidStart)
            );
            assert_eq!(
                Config::parse(Some("1"), Some(invalid)),
                Err(ConfigError::InvalidSectorCount)
            );
        }
    }

    #[test]
    fn excludes_boot_and_requires_room_for_multi_sector_transfer() {
        assert_eq!(
            Config::parse(Some("0"), Some("2")),
            Err(ConfigError::BootSectorExcluded)
        );
        for count in ["0", "1"] {
            assert_eq!(
                Config::parse(Some("1"), Some(count)),
                Err(ConfigError::TooFewSectors)
            );
        }
    }

    #[test]
    fn rejects_address_overflow_and_out_of_bounds_region() {
        assert_eq!(
            Config::parse(Some("4294967294"), Some("2")),
            Err(ConfigError::RegionOverflow)
        );
        let region = Config::parse(Some("100"), Some("10"))
            .unwrap()
            .scratch
            .unwrap();
        assert_eq!(region.validate_capacity(110), Ok(region));
        assert_eq!(
            region.validate_capacity(109),
            Err(ConfigError::BeyondCapacity)
        );
        assert_eq!(
            region.validate_capacity(0),
            Err(ConfigError::BeyondCapacity)
        );
        // Capacity validation also protects callers constructing a region directly.
        assert_eq!(
            ScratchRegion {
                start: 0,
                sectors: 2
            }
            .validate_capacity(100),
            Err(ConfigError::BootSectorExcluded)
        );
    }

    #[test]
    fn pattern_distinguishes_sector_addresses_and_byte_order() {
        let mut buffer = [0; 512];
        fill_pattern(&mut buffer, 255);
        assert_eq!(verify_pattern(&buffer, 255), Ok(()));
        for other in [0, 256, 511, 65_791, u32::MAX] {
            assert!(verify_pattern(&buffer, other).is_err());
        }
        buffer.swap(0, 1);
        assert!(verify_pattern(&buffer, 255).is_err());
    }

    #[test]
    fn pattern_checks_the_entire_sector_and_reports_corruption() {
        let mut buffer = [0; 512];
        fill_pattern(&mut buffer, 42);
        let expected = buffer[511];
        buffer[511] ^= 1;
        assert_eq!(
            verify_pattern(&buffer, 42),
            Err(PatternMismatch {
                offset: 511,
                expected,
                actual: expected ^ 1
            })
        );
    }

    #[test]
    fn fresh_seed_alternates_across_successful_runs() {
        let mut block = [0; 512];
        for sector in [0, 1, 255, 256, 0x1234_5678, u32::MAX] {
            fill_pattern(&mut block, sector);
            assert_eq!(fresh_seed(&block, sector), sector ^ u32::MAX);
            assert!(verify_pattern(&block, sector ^ u32::MAX).is_err());
            fill_pattern(&mut block, sector ^ u32::MAX);
            assert_eq!(fresh_seed(&block, sector), sector);
            assert!(verify_pattern(&block, sector).is_err());
        }
    }

    #[test]
    fn complemented_candidates_differ_for_every_address_byte() {
        for byte in 0..=255_u32 {
            let sector = byte * 0x0101_0101;
            for offset in 0..512 {
                assert_ne!(pattern_byte(sector, offset), pattern_byte(!sector, offset));
            }
        }
    }

    #[test]
    fn partial_multi_write_detects_either_stale_sector() {
        for omitted in 0..2 {
            let mut media = [[0; 512]; 2];
            // Model both sectors left by a previous successful probe run.
            for (i, block) in media.iter_mut().enumerate() {
                fill_pattern(block, 100 + i as u32);
            }
            let seeds = fresh_pair(&media[0], &media[1], 100);
            for (i, block) in media.iter_mut().enumerate() {
                if i != omitted {
                    fill_pattern(block, seeds[i]);
                }
            }
            for (i, block) in media.iter().enumerate() {
                assert_eq!(verify_pattern(block, seeds[i]).is_err(), i == omitted);
            }
        }
    }

    #[test]
    fn fresh_pair_handles_complement_boundary_and_aliases() {
        let start = 0x7fff_ffff;
        let mut media = [[0; 512]; 2];
        fill_pattern(&mut media[0], start);
        // Exercise both forbidden candidates for the second seed, as well as
        // its normal prior-run pattern at the complement boundary.
        for old_second in [0x8000_0000, 0x8000_0001, 0x8000_0002] {
            fill_pattern(&mut media[1], old_second);
            let seeds = fresh_pair(&media[0], &media[1], start);
            assert_ne!(seeds[0], seeds[1]);
            for i in 0..2 {
                assert!(verify_pattern(&media[i], seeds[i]).is_err());
            }
            let mut written = [[0; 512]; 2];
            for i in 0..2 {
                fill_pattern(&mut written[i], seeds[i]);
            }
            for alias_source in 0..2 {
                assert!(verify_pattern(&written[alias_source], seeds[1 - alias_source]).is_err());
            }
        }
    }
}

#[cfg(test)]
mod fault_tests {
    use super::*;

    #[test]
    fn fault_modes_fail_closed_and_busy_requires_write_authorization() {
        let read_only = Config::parse(None, None).unwrap();
        let writable = Config::parse(Some("2048"), Some("2")).unwrap();
        assert_eq!(Fault::parse(None, read_only), Ok(Fault::None));
        assert_eq!(
            Fault::parse(Some("missing-completion"), read_only),
            Ok(Fault::MissingCompletion)
        );
        assert_eq!(
            Fault::parse(Some("read-crc"), read_only),
            Ok(Fault::ReadCrc)
        );
        assert_eq!(
            Fault::parse(Some("write-busy"), writable),
            Ok(Fault::WriteBusy)
        );
        assert_eq!(
            Fault::parse(Some("write-busy"), read_only),
            Err("write_busy_requires_scratch")
        );
        for value in ["", "none", "write_busy", "read-crc "] {
            assert_eq!(Fault::parse(Some(value), writable), Err("invalid_fault"));
        }
    }
}
