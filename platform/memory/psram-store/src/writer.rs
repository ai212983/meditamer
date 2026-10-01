//! Ordered stream writer over a [`ChunkStore`](crate::ChunkStore).
//!
//! SD loaders stream pack bytes in order; the writer tracks the next
//! expected offset and rejects gaps, replays, and over-long streams, so a
//! transfer either lands contiguously or fails without moving the cursor.
//! The general store keeps its random-access API for other components.

use allocator_api2::alloc::Allocator;
use core::fmt;

use crate::ChunkStore;

/// Ordered-write failure. The writer cursor never advances on failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriterError {
    /// `write_at` targeted past the cursor; the bytes in between are missing.
    Gap { expected: usize, got: usize },
    /// `write_at` targeted behind the cursor; the range was already written.
    Overlap { expected: usize, got: usize },
    /// The bytes do not fit in the remaining store length (extra data).
    Overrun {
        offset: usize,
        len: usize,
        capacity: usize,
    },
    /// `finish` ran with fewer bytes than the declared exact length.
    Truncated { received: usize, expected: usize },
}

impl fmt::Display for WriterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Gap { expected, got } => {
                write!(
                    f,
                    "psram-store: gap at {got}, expected next offset {expected}"
                )
            }
            Self::Overlap { expected, got } => {
                write!(
                    f,
                    "psram-store: replay at {got}, expected next offset {expected}"
                )
            }
            Self::Overrun {
                offset,
                len,
                capacity,
            } => write!(
                f,
                "psram-store: {len} bytes at {offset} exceed store length {capacity}"
            ),
            Self::Truncated { received, expected } => write!(
                f,
                "psram-store: stream ended at {received} of {expected} bytes"
            ),
        }
    }
}

/// Ordered writer borrowing a [`ChunkStore`](crate::ChunkStore).
///
/// Created over a store sized either at the pack's exact length or at its
/// format maximum. Every write must start exactly at [`position`](Self::position);
/// pushes append there. With a known length, [`finish`](Self::finish)
/// requires exactly that many bytes. With a declared maximum,
/// [`finish_prefix`](Self::finish_prefix) returns the received count and
/// the caller (the pack codec) checks the actual length against the header.
pub struct SequentialWriter<'a, A: Allocator> {
    store: &'a mut ChunkStore<A>,
    next: usize,
}

impl<'a, A: Allocator> SequentialWriter<'a, A> {
    /// Start an ordered stream at offset zero of `store`.
    pub fn new(store: &'a mut ChunkStore<A>) -> Self {
        Self { store, next: 0 }
    }

    /// Next expected offset.
    pub fn position(&self) -> usize {
        self.next
    }

    /// Bytes still writable before the store end.
    pub fn remaining(&self) -> usize {
        self.store.len().saturating_sub(self.next)
    }

    /// Append `bytes` at the cursor.
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), WriterError> {
        self.write_at(self.next, bytes)
    }

    /// Write `bytes` at `offset`, which must equal the cursor: a larger
    /// offset is a [`Gap`](WriterError::Gap), a smaller one an
    /// [`Overlap`](WriterError::Overlap), and bytes past the store end an
    /// [`Overrun`](WriterError::Overrun).
    pub fn write_at(&mut self, offset: usize, bytes: &[u8]) -> Result<(), WriterError> {
        if offset != self.next {
            return Err(if offset > self.next {
                WriterError::Gap {
                    expected: self.next,
                    got: offset,
                }
            } else {
                WriterError::Overlap {
                    expected: self.next,
                    got: offset,
                }
            });
        }
        let end = offset
            .checked_add(bytes.len())
            .filter(|end| *end <= self.store.len())
            .ok_or(WriterError::Overrun {
                offset,
                len: bytes.len(),
                capacity: self.store.len(),
            })?;
        // The bounds check above makes a store failure impossible; map it
        // to `Overrun` with the actual request shape rather than fabricating
        // offsets.
        self.store
            .write_at(offset, bytes)
            .map_err(|_| WriterError::Overrun {
                offset,
                len: bytes.len(),
                capacity: self.store.len(),
            })?;
        self.next = end;
        Ok(())
    }

    /// Complete an exact-length stream; fails with
    /// [`Truncated`](WriterError::Truncated) unless every store byte arrived.
    pub fn finish(self) -> Result<(), WriterError> {
        if self.next != self.store.len() {
            return Err(WriterError::Truncated {
                received: self.next,
                expected: self.store.len(),
            });
        }
        Ok(())
    }

    /// Complete a maximum-reserved stream, returning the received byte
    /// count for the codec to validate against the pack header.
    pub fn finish_prefix(self) -> usize {
        self.next
    }
}
