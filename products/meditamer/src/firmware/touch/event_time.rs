//! Full-width event time with four-byte alignment for bounded queue storage.
//!
//! Adding admission metadata to an eight-byte-aligned event would add eight
//! bytes per queue slot. Two words retain every timestamp bit without packed
//! fields, unaligned references, or changing the queue's capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EventTime {
    low: u32,
    high: u32,
}

impl EventTime {
    pub(crate) const fn new(milliseconds: u64) -> Self {
        Self {
            low: milliseconds as u32,
            high: (milliseconds >> 32) as u32,
        }
    }

    pub(crate) const fn milliseconds(self) -> u64 {
        (self.high as u64) << 32 | self.low as u64
    }
}
