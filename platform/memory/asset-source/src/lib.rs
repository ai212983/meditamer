//! Generic checked read/borrow access core for bulk asset bytes.
//!
//! [`AssetSource`] is a read-only random-access contract over one logical
//! byte string. [`AssetSource::read_exact_at`] copies any valid range (crossing
//! span boundaries for fragmented stores), while [`AssetSource::borrow_at`]
//! exposes a zero-copy borrow only when the requested range is contiguous.
//! A `None` borrow is never an error: it means a valid range that is not
//! contiguous in memory, and the caller falls back to `read_exact_at`.
//! All range arithmetic is fully checked: wrapping addition reports
//! [`AccessError::Overflow`] before any capacity comparison, and a checked
//! end past capacity reports [`AccessError::OutOfBounds`]. An empty range at
//! exactly the capacity is valid for both operations.

#![no_std]

/// Failure modes for checked access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessError {
    /// A checked offset or total-length addition overflowed.
    Overflow,
    /// A checked end lies past capacity. `offset + len` itself did not
    /// overflow; the range is simply outside the logical byte string.
    OutOfBounds {
        /// Requested start offset.
        offset: usize,
        /// Requested length.
        len: usize,
        /// Logical capacity the range was checked against.
        capacity: usize,
    },
}

/// Single fully checked range-end computation shared by every bounds check
/// below, including total-length accumulation in [`FragmentedSource::new`].
fn checked_end(offset: usize, len: usize, capacity: usize) -> Result<usize, AccessError> {
    let end = offset.checked_add(len).ok_or(AccessError::Overflow)?;
    if end > capacity {
        return Err(AccessError::OutOfBounds {
            offset,
            len,
            capacity,
        });
    }
    Ok(end)
}

/// Read-only random access over one logical byte string.
pub trait AssetSource {
    /// Access failure; both bundled adapters use [`AccessError`].
    type Error;
    /// Logical byte length of the source.
    fn len(&self) -> usize;
    /// Reports whether the source holds no bytes.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Copies exactly `dst.len()` bytes starting at `offset`. An empty `dst`
    /// at exactly `len()` succeeds and copies nothing.
    fn read_exact_at(&self, offset: usize, dst: &mut [u8]) -> Result<(), Self::Error>;
    /// Borrows `len` bytes starting at `offset`. Returns `None` when the
    /// range is valid but not contiguous in memory (only a fragmented source
    /// spanning more than one span can do this); the caller then falls back
    /// to [`AssetSource::read_exact_at`]. An empty range at exactly `len()`
    /// succeeds with `Some(&[])`.
    fn borrow_at(&self, offset: usize, len: usize) -> Result<Option<&[u8]>, Self::Error>;
}

/// Contiguous adapter over one borrowed slice. Every valid range is
/// contiguous, so [`AssetSource::borrow_at`] never returns `None` here.
#[derive(Clone, Copy, Debug)]
pub struct SliceSource<'a> {
    bytes: &'a [u8],
}

impl<'a> SliceSource<'a> {
    /// Wraps `bytes`. The total length is `bytes.len()`, so no addition runs
    /// and construction cannot overflow.
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }
}

impl AssetSource for SliceSource<'_> {
    type Error = AccessError;

    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn read_exact_at(&self, offset: usize, dst: &mut [u8]) -> Result<(), AccessError> {
        let end = checked_end(offset, dst.len(), self.bytes.len())?;
        dst.copy_from_slice(&self.bytes[offset..end]);
        Ok(())
    }

    fn borrow_at(&self, offset: usize, len: usize) -> Result<Option<&[u8]>, AccessError> {
        let end = checked_end(offset, len, self.bytes.len())?;
        Ok(Some(&self.bytes[offset..end]))
    }
}

/// Fragmented adapter over borrowed spans. The logical byte string is the
/// spans concatenated in order; empty spans contribute no bytes and are
/// skipped. [`AssetSource::read_exact_at`] copies across span boundaries,
/// while [`AssetSource::borrow_at`] returns `Some` only when the whole range
/// lies inside a single span and `None` for a valid range crossing spans.
#[derive(Clone, Copy, Debug)]
pub struct FragmentedSource<'a> {
    spans: &'a [&'a [u8]],
    total: usize,
}

impl<'a> FragmentedSource<'a> {
    /// Wraps `spans`. Totals accumulate with the same `checked_end` helper
    /// used by every access path (`checked_end(total, span.len(),
    /// usize::MAX)`), so a total-length overflow would surface as
    /// [`AccessError::Overflow`]. It cannot be constructed from real slices
    /// on current targets without unsafe fake slices, which this crate never
    /// uses; the unit tests therefore exercise that path through
    /// `checked_end` directly.
    pub fn new(spans: &'a [&'a [u8]]) -> Result<Self, AccessError> {
        let mut total = 0;
        for span in spans {
            total = checked_end(total, span.len(), usize::MAX)?;
        }
        Ok(Self { spans, total })
    }
}

impl AssetSource for FragmentedSource<'_> {
    type Error = AccessError;

    fn len(&self) -> usize {
        self.total
    }

    fn read_exact_at(&self, offset: usize, dst: &mut [u8]) -> Result<(), AccessError> {
        let end = checked_end(offset, dst.len(), self.total)?;
        let mut cursor: usize = 0;
        let mut written: usize = 0;
        for span in self.spans {
            let span_end = cursor
                .checked_add(span.len())
                .ok_or(AccessError::Overflow)?;
            if offset < span_end && cursor < end {
                let from = offset.saturating_sub(cursor);
                let to = if end < span_end {
                    end - cursor
                } else {
                    span.len()
                };
                dst[written..written + (to - from)].copy_from_slice(&span[from..to]);
                written += to - from;
            }
            cursor = span_end;
        }
        Ok(())
    }

    fn borrow_at(&self, offset: usize, len: usize) -> Result<Option<&[u8]>, AccessError> {
        let end = checked_end(offset, len, self.total)?;
        if len == 0 {
            return Ok(Some(&[]));
        }
        let mut cursor: usize = 0;
        for span in self.spans {
            let span_end = cursor
                .checked_add(span.len())
                .ok_or(AccessError::Overflow)?;
            if offset >= cursor && end <= span_end {
                let from = offset - cursor;
                return Ok(Some(&span[from..from + len]));
            }
            cursor = span_end;
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
