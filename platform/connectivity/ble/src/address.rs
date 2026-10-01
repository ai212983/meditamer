//! Legacy `ADV_IND` random-static advertising address generation (shared
//! BLE runtime and roles plan, Phase 4 increment 3: "legacy `ADV_IND`
//! address-rotation rules... as host-testable pure logic").
//!
//! ADR-0011: "Each confirmed controller-power epoch gets a new
//! random-static address. Its two most significant bits are `11`; valid
//! generation excludes all-zero, all-one, and immediately repeated random
//! parts. The address stays fixed within the epoch and telemetry omits
//! it." [`generate_epoch_address`] is the single place that rule lives:
//! given 48 bits of caller-supplied entropy (the chip's RNG on-target, a
//! fixed byte pattern in a host test) and the previous epoch's address (if
//! any), it either returns the new epoch's address or says which rule the
//! entropy failed, leaving retry with fresh entropy to the caller.
//!
//! Two things ADR-0011 says about addresses are deliberately *not* this
//! module's job: "telemetry omits it" is a logging-site policy, not a
//! generation-time constraint, and "ambiguous teardown blocks address
//! rotation and another window" is a sequencing precondition the caller
//! (increment 4's peripheral wiring) enforces by simply not calling this
//! function after a failed teardown -- this function has no teardown state
//! to consult.

/// A Bluetooth device address is six bytes.
pub const ADDRESS_BYTES: usize = 6;

/// The two most significant bits of a random-static address's first byte,
/// fixed by the Bluetooth Core specification's address-type encoding, not
/// a value this crate chose.
const ADDRESS_TYPE_MASK: u8 = 0b1100_0000;
const ADDRESS_TYPE_BITS: u8 = 0b1100_0000;

/// One epoch's legal random-static advertising address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EpochAddress([u8; ADDRESS_BYTES]);

impl EpochAddress {
    pub const fn bytes(&self) -> [u8; ADDRESS_BYTES] {
        self.0
    }
}

/// Why [`generate_epoch_address`] refused a candidate. The caller should
/// draw fresh entropy and retry -- none of these are permanent failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressGenerationError {
    /// The 46-bit random part (everything but the two type bits) was
    /// entirely zero.
    AllZeroRandomPart,
    /// The 46-bit random part was entirely one.
    AllOneRandomPart,
    /// The candidate is byte-for-byte identical to the previous epoch's
    /// address.
    RepeatsPreviousEpoch,
}

/// Builds one epoch's random-static advertising address from 48 bits of
/// entropy. The two most significant bits are forced to `11` regardless of
/// what the entropy carries there -- a caller's RNG is not expected to
/// already produce a legal Bluetooth static-address type marker, so this
/// function supplies it rather than validating for it. The remaining 46
/// bits are the "random part" ADR-0011 constrains: reject an all-zero or
/// all-one random part, or one that exactly repeats `previous`.
pub fn generate_epoch_address(
    entropy: [u8; ADDRESS_BYTES],
    previous: Option<EpochAddress>,
) -> Result<EpochAddress, AddressGenerationError> {
    let mut candidate = entropy;
    candidate[0] = (candidate[0] & !ADDRESS_TYPE_MASK) | ADDRESS_TYPE_BITS;

    let random_part_first_byte = candidate[0] & !ADDRESS_TYPE_MASK;
    let random_part_all_zero =
        random_part_first_byte == 0 && candidate[1..].iter().all(|&byte| byte == 0);
    if random_part_all_zero {
        return Err(AddressGenerationError::AllZeroRandomPart);
    }
    let random_part_all_one = random_part_first_byte == !ADDRESS_TYPE_MASK
        && candidate[1..].iter().all(|&byte| byte == 0xff);
    if random_part_all_one {
        return Err(AddressGenerationError::AllOneRandomPart);
    }
    if let Some(previous) = previous {
        if candidate == previous.0 {
            return Err(AddressGenerationError::RepeatsPreviousEpoch);
        }
    }
    Ok(EpochAddress(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_bits_are_forced_regardless_of_entropy() {
        for top_bits in [0b00, 0b01, 0b10, 0b11] {
            let mut entropy = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
            entropy[0] = (entropy[0] & 0b0011_1111) | (top_bits << 6);
            let address =
                generate_epoch_address(entropy, None).expect("non-degenerate random part");
            assert_eq!(address.bytes()[0] & ADDRESS_TYPE_MASK, ADDRESS_TYPE_BITS);
        }
    }

    #[test]
    fn random_part_survives_unchanged_below_the_type_bits() {
        let entropy = [0b0011_1001, 0xaa, 0xbb, 0xcc, 0xdd, 0xee];
        let address = generate_epoch_address(entropy, None).expect("non-degenerate");
        assert_eq!(address.bytes()[0], 0b1111_1001);
        assert_eq!(&address.bytes()[1..], &[0xaa, 0xbb, 0xcc, 0xdd, 0xee]);
    }

    #[test]
    fn all_zero_random_part_is_rejected() {
        let entropy = [0x00; ADDRESS_BYTES];
        assert_eq!(
            generate_epoch_address(entropy, None),
            Err(AddressGenerationError::AllZeroRandomPart)
        );
    }

    #[test]
    fn all_one_random_part_is_rejected() {
        let entropy = [0xff; ADDRESS_BYTES];
        assert_eq!(
            generate_epoch_address(entropy, None),
            Err(AddressGenerationError::AllOneRandomPart)
        );
    }

    #[test]
    fn almost_all_zero_random_part_is_accepted() {
        let mut entropy = [0x00; ADDRESS_BYTES];
        entropy[5] = 0x01;
        assert!(generate_epoch_address(entropy, None).is_ok());
    }

    #[test]
    fn almost_all_one_random_part_is_accepted() {
        let mut entropy = [0xff; ADDRESS_BYTES];
        entropy[5] = 0xfe;
        assert!(generate_epoch_address(entropy, None).is_ok());
    }

    #[test]
    fn zero_entropy_with_type_bits_already_set_is_still_rejected() {
        // The type bits, once forced, make the stored first byte 0xC0 --
        // but the *random part* (the low six bits) is still zero, so this
        // must be treated the same as fully-zero entropy.
        let entropy = [0b1100_0000, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(
            generate_epoch_address(entropy, None),
            Err(AddressGenerationError::AllZeroRandomPart)
        );
    }

    #[test]
    fn repeating_the_previous_epoch_address_is_rejected() {
        let entropy = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let first = generate_epoch_address(entropy, None).expect("first epoch");
        assert_eq!(
            generate_epoch_address(entropy, Some(first)),
            Err(AddressGenerationError::RepeatsPreviousEpoch)
        );
    }

    #[test]
    fn distinct_entropy_after_a_previous_epoch_is_accepted() {
        let first = generate_epoch_address([0x11, 0x22, 0x33, 0x44, 0x55, 0x66], None)
            .expect("first epoch");
        let second = generate_epoch_address([0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc], Some(first))
            .expect("second epoch");
        assert_ne!(first, second);
    }
}
