//! Allocation-free, ordered layout of one named phase bundle.
//!
//! This is a structural requirement, not a shared-heap free-space probe or
//! proof that physical backing has been acquired.

use crate::ResourceId;

/// Identity of one named allocation set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BundleId(pub u16);

/// One required allocation in a named set, in allocation order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BundlePart {
    pub resource: ResourceId,
    pub bytes: usize,
    pub align: usize,
}

/// Borrowed, allocation-free declaration of a named allocation set. A product
/// may use several sets for distinct lifetimes; this type alone does not
/// claim the entire screen working set or physical backing.
#[derive(Clone, Copy, Debug)]
pub struct Bundle<'a> {
    pub id: BundleId,
    pub version: u16,
    pub parts: &'a [BundlePart],
}

/// Exact bytes needed by the ordered parts, including alignment padding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BundleLayout {
    pub bytes: usize,
}

/// A declaration cannot be laid out safely.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BundleError {
    Empty,
    InvalidIdentity,
    InvalidVersion,
    InvalidSize,
    InvalidAlignment,
    DuplicateResource,
    Overflow,
}

impl Bundle<'_> {
    /// Validate identity, uniqueness and alignment, then compute the whole
    /// ordered layout without changing any allocator state.
    pub fn layout(&self, max_align: usize) -> Result<BundleLayout, BundleError> {
        if self.id.0 == 0 || self.parts.iter().any(|part| part.resource.0 == 0) {
            return Err(BundleError::InvalidIdentity);
        }
        if self.version == 0 {
            return Err(BundleError::InvalidVersion);
        }
        if self.parts.is_empty() {
            return Err(BundleError::Empty);
        }
        if max_align == 0 || !max_align.is_power_of_two() {
            return Err(BundleError::InvalidAlignment);
        }
        let mut end = 0usize;
        for (index, part) in self.parts.iter().enumerate() {
            if part.bytes == 0 {
                return Err(BundleError::InvalidSize);
            }
            if part.align == 0 || !part.align.is_power_of_two() || part.align > max_align {
                return Err(BundleError::InvalidAlignment);
            }
            if self.parts[..index]
                .iter()
                .any(|prior| prior.resource == part.resource)
            {
                return Err(BundleError::DuplicateResource);
            }
            let mask = part.align - 1;
            let padding = (part.align - (end & mask)) & mask;
            end = end
                .checked_add(padding)
                .and_then(|aligned| aligned.checked_add(part.bytes))
                .ok_or(BundleError::Overflow)?;
        }
        Ok(BundleLayout { bytes: end })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_for_padding_and_exact_fit() {
        let parts = [
            BundlePart {
                resource: ResourceId(1),
                bytes: 3,
                align: 1,
            },
            BundlePart {
                resource: ResourceId(2),
                bytes: 4,
                align: 4,
            },
        ];
        assert_eq!(
            Bundle {
                id: BundleId(1),
                version: 1,
                parts: &parts
            }
            .layout(4),
            Ok(BundleLayout { bytes: 8 })
        );
    }

    #[test]
    fn rejects_duplicates_and_overflow() {
        let same = [BundlePart {
            resource: ResourceId(1),
            bytes: 1,
            align: 1,
        }; 2];
        assert_eq!(
            Bundle {
                id: BundleId(1),
                version: 1,
                parts: &same
            }
            .layout(4),
            Err(BundleError::DuplicateResource)
        );
        let huge = [
            BundlePart {
                resource: ResourceId(1),
                bytes: 1,
                align: 1,
            },
            BundlePart {
                resource: ResourceId(2),
                bytes: usize::MAX,
                align: 4,
            },
        ];
        assert_eq!(
            Bundle {
                id: BundleId(1),
                version: 1,
                parts: &huge
            }
            .layout(4),
            Err(BundleError::Overflow)
        );
    }
}
