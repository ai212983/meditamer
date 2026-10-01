//! Checked, allocation-free asset access over a resident chunk store.

use allocator_api2::alloc::Allocator;
use asset_source::AssetSource;

use crate::{AccessError, ChunkStore};

impl<A: Allocator> AssetSource for ChunkStore<A> {
    type Error = AccessError;

    fn len(&self) -> usize {
        ChunkStore::len(self)
    }

    fn read_exact_at(&self, offset: usize, dst: &mut [u8]) -> Result<(), Self::Error> {
        ChunkStore::read_exact_at(self, offset, dst)
    }

    fn borrow_at(&self, offset: usize, len: usize) -> Result<Option<&[u8]>, Self::Error> {
        let end = offset.checked_add(len).ok_or(AccessError::Overflow)?;
        if end > self.len() {
            return Err(AccessError::OutOfBounds {
                offset,
                len,
                capacity: self.len(),
            });
        }
        Ok(self.slice_at(offset, len))
    }
}
