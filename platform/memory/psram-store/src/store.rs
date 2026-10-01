//! Bounded chunked byte store backed by caller-supplied PSRAM.
//!
//! A [`ChunkStore`] owns a logical byte length split into one or more
//! physical chunks of at most [`ChunkLimits::max_chunk_bytes`] bytes; only
//! the final chunk may be smaller. Both chunk data and chunk metadata
//! allocate through the single target-supplied allocator instance, so the
//! store never touches internal RAM, the global allocator, or any product
//! state. Allocation is fully fallible: a failed construction frees every
//! chunk it already claimed and reports the shape it reached.

use allocator_api2::alloc::{AllocError, Allocator, Layout};
use allocator_api2::vec::Vec;
use core::fmt;
use core::ptr::NonNull;
use core::slice;

/// Target-measured bounds for one [`ChunkStore`](crate::ChunkStore).
///
/// Each target selects these from device measurements against its usable
/// PSRAM budget and peak asset bundle (active packs plus render caches and
/// the shared canvas). A request beyond any bound is a [`ChunkStoreError`],
/// never a panic; rerun the load, reentry, and upload memory checks if a
/// value changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkLimits {
    /// Largest physical allocation the store may request, in bytes.
    pub max_chunk_bytes: usize,
    /// Largest number of physical chunks one store may own.
    pub max_chunks: usize,
    /// Largest logical store length, in bytes.
    pub max_total_bytes: usize,
}

impl ChunkLimits {
    /// Bundle the three target-measured bounds.
    pub const fn new(max_chunk_bytes: usize, max_chunks: usize, max_total_bytes: usize) -> Self {
        Self {
            max_chunk_bytes,
            max_chunks,
            max_total_bytes,
        }
    }

    /// Number of physical chunks a `len`-byte store needs under these
    /// bounds, without allocating.
    pub fn chunk_count_for(&self, len: usize) -> Result<usize, ChunkStoreError> {
        if self.max_chunk_bytes == 0 || Layout::from_size_align(self.max_chunk_bytes, 1).is_err() {
            return Err(ChunkStoreError::InvalidLimits);
        }
        if len > self.max_total_bytes {
            return Err(ChunkStoreError::TooLong {
                len,
                max_total_bytes: self.max_total_bytes,
            });
        }
        let chunks = len.div_ceil(self.max_chunk_bytes);
        if chunks > self.max_chunks {
            return Err(ChunkStoreError::TooManyChunks {
                chunks,
                max_chunks: self.max_chunks,
            });
        }
        Ok(chunks)
    }
}

/// Construction failure for [`ChunkStore`](crate::ChunkStore).
///
/// Every variant carries the request shape so the caller can record
/// requested size, chunk index/size, and allocation failure per the plan.
/// A failed construction frees all partial state before returning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkStoreError {
    /// The target bounds cannot back any store (`max_chunk_bytes` is zero
    /// or not representable as a [`Layout`](core::alloc::Layout) size).
    InvalidLimits,
    /// `len` exceeds the target's total store budget.
    TooLong { len: usize, max_total_bytes: usize },
    /// `len` needs more chunks than the target allows.
    TooManyChunks { chunks: usize, max_chunks: usize },
    /// Allocation failed partway; the partial store was released.
    AllocFailed { allocated: usize, requested: usize },
}

impl fmt::Display for ChunkStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::InvalidLimits => write!(f, "psram-store: chunk limits cannot back any store"),
            Self::TooLong {
                len,
                max_total_bytes,
            } => write!(
                f,
                "psram-store: length {len} exceeds total budget {max_total_bytes}"
            ),
            Self::TooManyChunks { chunks, max_chunks } => write!(
                f,
                "psram-store: {chunks} chunks exceed chunk budget {max_chunks}"
            ),
            Self::AllocFailed {
                allocated,
                requested,
            } => write!(
                f,
                "psram-store: allocation failed after {allocated} of {requested} chunks"
            ),
        }
    }
}

/// Checked access failure. Runtime failures never change the store layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessError {
    /// `offset + len` overflowed `usize`.
    Overflow,
    /// The range reaches past the logical length.
    OutOfBounds {
        offset: usize,
        len: usize,
        capacity: usize,
    },
}

impl fmt::Display for AccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Overflow => write!(f, "psram-store: offset plus length overflowed"),
            Self::OutOfBounds {
                offset,
                len,
                capacity,
            } => write!(
                f,
                "psram-store: range [{offset}, {offset}+{len}) exceeds length {capacity}"
            ),
        }
    }
}

/// One owned physical chunk: PSRAM allocated through the store allocator.
#[derive(Debug)]
struct Chunk {
    ptr: NonNull<u8>,
    layout: Layout,
}

// SAFETY: a `Chunk` solely owns its allocation (like an allocator-api2
// `Box<[u8]>`): transferring ownership to another thread transfers the free
// responsibility, and no thread retains access to the old owner.
unsafe impl Send for Chunk {}

// SAFETY: shared access to a `Chunk` only exposes shared byte slices of a
// live allocation owned by the store, so sharing across threads is as safe
// as sharing the bytes themselves. No `ChunkStore` method offers shared
// mutation. The enclosing `ChunkStore<A>` keeps its conditional
// `A: Send` / `A: Sync` bounds via `Vec<Chunk, A>`.
unsafe impl Sync for Chunk {}

impl Chunk {
    fn len(&self) -> usize {
        self.layout.size()
    }

    /// Borrow the chunk bytes. Safe while the store owns the allocation
    /// and no mutable access overlaps the borrow.
    unsafe fn as_slice(&self) -> &[u8] {
        // SAFETY: the pointer comes from a live allocation of `layout`
        // owned by the store; the caller upholds borrow rules.
        unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.len()) }
    }

    /// Mutably borrow the chunk bytes under the same ownership contract.
    unsafe fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: as above, with exclusive access through `&mut self`.
        unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len()) }
    }
}

/// Reusable byte store split into bounded PSRAM chunks.
///
/// The logical length is fixed at construction and zero-initialized, so
/// random [`write_at`](Self::write_at) is valid from the start. Dropping
/// the store frees every chunk and the chunk metadata.
#[derive(Debug)]
pub struct ChunkStore<A: Allocator> {
    chunks: Vec<Chunk, A>,
    len: usize,
    max_chunk_bytes: usize,
}

impl<A: Allocator> ChunkStore<A> {
    /// Allocate a zeroed `len`-byte store under `limits` through `alloc`.
    ///
    /// Data chunks hold at most `limits.max_chunk_bytes` bytes; only the
    /// final chunk is smaller, and a request within that cap uses exactly
    /// one data allocation. Chunk metadata is reserved up front through
    /// the same allocator. Any allocation failure releases the partial
    /// store and returns [`ChunkStoreError::AllocFailed`].
    pub fn try_new_zeroed(
        len: usize,
        limits: ChunkLimits,
        alloc: A,
    ) -> Result<Self, ChunkStoreError> {
        let requested = limits.chunk_count_for(len)?;
        // The single allocator instance lives in `chunks`; data chunks
        // allocate through `chunks.allocator()` (`&A: Allocator`), so no
        // `Clone` bound is needed and unit allocators such as
        // `esp_alloc::ExternalMemory` work directly.
        let mut chunks = Vec::new_in(alloc);
        chunks
            .try_reserve_exact(requested)
            .map_err(|_| ChunkStoreError::AllocFailed {
                allocated: 0,
                requested,
            })?;
        let mut allocated = 0;
        while allocated < requested {
            let start = allocated * limits.max_chunk_bytes;
            let chunk_len = (len - start).min(limits.max_chunk_bytes);
            let layout = Layout::from_size_align(chunk_len.max(1), 1).map_err(|_| {
                free_chunks(chunks.allocator(), chunks.iter());
                ChunkStoreError::InvalidLimits
            })?;
            // Zeroed so random writes are valid from construction.
            let memory = chunks
                .allocator()
                .allocate_zeroed(layout)
                .map_err(|_: AllocError| {
                    free_chunks(chunks.allocator(), chunks.iter());
                    ChunkStoreError::AllocFailed {
                        allocated,
                        requested,
                    }
                })?;
            chunks.push(Chunk {
                ptr: memory.cast::<u8>(),
                layout,
            });
            allocated += 1;
        }
        Ok(Self {
            chunks,
            len,
            max_chunk_bytes: limits.max_chunk_bytes,
        })
    }

    /// Logical length in bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the logical length is zero (no physical chunks).
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Number of owned physical chunks (zero for an empty store).
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Length of physical chunk `index`, or `None` when out of range.
    pub fn chunk_len(&self, index: usize) -> Option<usize> {
        self.chunks.get(index).map(Chunk::len)
    }

    /// Largest physical chunk this store was built with.
    pub fn max_chunk_bytes(&self) -> usize {
        self.max_chunk_bytes
    }

    /// Borrow each physical chunk in logical order.
    pub fn spans(&self) -> Spans<'_, A> {
        Spans {
            chunks: self.chunks.iter(),
            _marker: core::marker::PhantomData,
        }
    }

    /// Borrow the requested range when it lies within one physical chunk.
    ///
    /// Returns `None` for out-of-bounds ranges and for ranges crossing a
    /// chunk boundary; use [`read_exact_at`](Self::read_exact_at) or
    /// [`spans`](Self::spans) for those.
    pub fn slice_at(&self, offset: usize, len: usize) -> Option<&[u8]> {
        if len == 0 {
            return if offset <= self.len { Some(&[]) } else { None };
        }
        let end = offset.checked_add(len)?;
        if end > self.len {
            return None;
        }
        // All chunks except the last have identical length, so a logical
        // offset identifies its physical chunk without scanning metadata.
        let index = offset / self.max_chunk_bytes;
        let within = offset % self.max_chunk_bytes;
        let chunk = &self.chunks[index];
        if len > chunk.len() - within {
            return None;
        }
        // SAFETY: the checked range lies inside this owned chunk.
        let bytes = unsafe { chunk.as_slice() };
        Some(&bytes[within..within + len])
    }

    /// Copy `dst.len()` bytes starting at `offset` into `dst`.
    pub fn read_exact_at(&self, offset: usize, dst: &mut [u8]) -> Result<(), AccessError> {
        checked_end(offset, dst.len(), self.len)?;
        let mut filled = 0;
        while filled < dst.len() {
            let position = offset + filled;
            let chunk = &self.chunks[position / self.max_chunk_bytes];
            let within = position % self.max_chunk_bytes;
            let take = (chunk.len() - within).min(dst.len() - filled);
            // SAFETY: the checked range lies inside this owned chunk.
            let bytes = unsafe { chunk.as_slice() };
            dst[filled..filled + take].copy_from_slice(&bytes[within..within + take]);
            filled += take;
        }
        Ok(())
    }

    /// Copy `src` into the store starting at `offset`, possibly crossing
    /// any number of chunk boundaries.
    pub fn write_at(&mut self, offset: usize, src: &[u8]) -> Result<(), AccessError> {
        checked_end(offset, src.len(), self.len)?;
        let mut written = 0;
        while written < src.len() {
            let position = offset + written;
            let chunk = &mut self.chunks[position / self.max_chunk_bytes];
            let within = position % self.max_chunk_bytes;
            let take = (chunk.len() - within).min(src.len() - written);
            // SAFETY: the checked range lies inside this exclusively owned chunk.
            let bytes = unsafe { chunk.as_mut_slice() };
            bytes[within..within + take].copy_from_slice(&src[written..written + take]);
            written += take;
        }
        Ok(())
    }
}

/// Release data chunks without touching chunk metadata ownership.
fn free_chunks<'a, A: Allocator>(alloc: &'a A, chunks: impl Iterator<Item = &'a Chunk>) {
    for chunk in chunks {
        // SAFETY: each chunk denotes a block currently allocated via this
        // allocator with its stored layout, and is deallocated exactly once
        // (either here on the failure path or in `Drop`).
        unsafe {
            alloc.deallocate(chunk.ptr, chunk.layout);
        }
    }
}

impl<A: Allocator> Drop for ChunkStore<A> {
    fn drop(&mut self) {
        free_chunks(self.chunks.allocator(), self.chunks.iter());
        // `chunks` (the metadata `Vec`) frees itself through its allocator.
    }
}

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

/// Borrowed physical-span iterator from [`ChunkStore::spans`].
pub struct Spans<'a, A: Allocator> {
    chunks: slice::Iter<'a, Chunk>,
    _marker: core::marker::PhantomData<&'a A>,
}

impl<'a, A: Allocator> Iterator for Spans<'a, A> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        self.chunks.next().map(|chunk| {
            // SAFETY: the chunk is owned by the borrowed store, which
            // outlives the iterator; no mutable access overlaps it.
            unsafe { chunk.as_slice() }
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.chunks.size_hint()
    }
}

impl<A: Allocator> ExactSizeIterator for Spans<'_, A> {}
