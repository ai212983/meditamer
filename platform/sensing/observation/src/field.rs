//! Provider-defined field sets.
//!
//! The subscription, demand, and delivery contract stays generic over
//! whatever fields a provider measures -- Medinote's SHTC3 defines
//! temperature and humidity, Meditamer's BQ27441 defines only battery level
//! -- by asking each provider for a small bitmask view rather than an enum
//! the core would have to know about. A provider implements [`FieldMask`]
//! for its own `Copy` field-set type (typically a `bitflags`-style newtype
//! over an integer) and the rest of this crate operates through that.

/// A conservative field-count suggestion for a provider that hasn't measured
/// its own need. Real providers here have one to two fields (temperature and
/// humidity, or level alone); every fixed-size per-field array in this crate
/// (`Demand`, `FieldTimestamps`) takes its capacity as an explicit
/// `const FIELDS: usize` a target chooses at its own call site instead, per
/// the DRAM budget's target-owned-capacity policy ([`crate::subscription`]'s
/// `SubscriptionBook<F, N>` and [`crate::observe_now`]'s
/// `ObserveNowQueue<F, N>` already follow this for subscription/request
/// counts) -- this constant is documentation, not a bound anything enforces.
pub const SUGGESTED_MAX_FIELDS: usize = 4;

/// A provider-defined bitmask of requestable/measurable fields.
///
/// Implementors are expected to be a thin `Copy` newtype over an unsigned
/// integer with one bit per field, matching the conventional `bitflags`
/// shape; this trait only asks for the bit-level operations the core needs.
pub trait FieldMask: Copy + Eq {
    /// No fields set.
    const EMPTY: Self;

    fn bits(self) -> u32;
    fn from_bits_truncate(bits: u32) -> Self;

    fn union(self, other: Self) -> Self {
        Self::from_bits_truncate(self.bits() | other.bits())
    }

    fn intersection(self, other: Self) -> Self {
        Self::from_bits_truncate(self.bits() & other.bits())
    }

    /// Whether every field set in `other` is also set in `self`.
    fn contains(self, other: Self) -> bool {
        self.bits() & other.bits() == other.bits()
    }

    fn intersects(self, other: Self) -> bool {
        self.bits() & other.bits() != 0
    }

    fn is_empty(self) -> bool {
        self.bits() == 0
    }
}

/// Iterates the set-bit indices of `mask`, low to high. Used to walk a
/// provider's field bits without the trait needing an enumeration method of
/// its own.
pub fn field_indices(mask: u32) -> impl Iterator<Item = usize> {
    let mut remaining = mask;
    core::iter::from_fn(move || {
        if remaining == 0 {
            None
        } else {
            let index = remaining.trailing_zeros() as usize;
            remaining &= remaining - 1;
            Some(index)
        }
    })
}

#[cfg(test)]
pub(crate) mod test_fields {
    use super::FieldMask;

    /// A two-bit test field set standing in for a real provider's fields
    /// (e.g. SHTC3 temperature/humidity) across this crate's unit tests.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct TestFields(pub u32);

    impl TestFields {
        pub const TEMPERATURE: TestFields = TestFields(1 << 0);
        pub const HUMIDITY: TestFields = TestFields(1 << 1);
    }

    impl FieldMask for TestFields {
        const EMPTY: Self = TestFields(0);

        fn bits(self) -> u32 {
            self.0
        }

        fn from_bits_truncate(bits: u32) -> Self {
            TestFields(bits & (Self::TEMPERATURE.0 | Self::HUMIDITY.0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_fields::TestFields;
    use super::*;

    #[test]
    fn union_and_contains() {
        let both = TestFields::TEMPERATURE.union(TestFields::HUMIDITY);
        assert!(both.contains(TestFields::TEMPERATURE));
        assert!(both.contains(TestFields::HUMIDITY));
        assert!(!TestFields::TEMPERATURE.contains(TestFields::HUMIDITY));
    }

    #[test]
    fn from_bits_truncate_drops_unknown_bits() {
        let truncated = TestFields::from_bits_truncate(0xFFFF_FFFF);
        assert_eq!(
            truncated,
            TestFields::TEMPERATURE.union(TestFields::HUMIDITY)
        );
    }

    #[test]
    fn field_indices_walks_low_to_high() {
        let mask = (1 << 3) | (1 << 5) | (1 << 0);
        let indices: heapless::Vec<usize, 4> = field_indices(mask).collect();
        assert_eq!(indices.as_slice(), &[0, 3, 5]);
    }

    #[test]
    fn empty_mask_has_no_indices() {
        assert_eq!(field_indices(0).count(), 0);
        assert!(TestFields::EMPTY.is_empty());
    }
}
