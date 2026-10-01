//! Bounded chunked byte store for PSRAM-backed bulk assets.
//!
//! [`ChunkStore`] owns a logical length split into bounded physical chunks
//! allocated through one target-supplied allocator, so bulk packs (clock,
//! mountain, sky/sun) stop repeating hand-written chunk machinery and stop
//! depending on whole-pack contiguous fits. [`SequentialWriter`] adds the
//! ordered stream contract SD loaders need. Pack codecs keep header,
//! geometry, exact-length, and CRC authority; screens own the validated
//! store lifetime.
//!
//! ```text
//! let limits = ChunkLimits::new(max_chunk_bytes, max_chunks, max_total_bytes);
//! let mut store = ChunkStore::try_new_zeroed(pack_len, limits, psram_alloc)?;
//! let mut stream = SequentialWriter::new(&mut store);
//! stream.push(&first_read)?;
//! // ...
//! stream.finish()?; // exact length; or finish_prefix() at a format maximum
//! ```

#![no_std]

extern crate alloc;

mod source;
mod store;
mod writer;

pub use store::{AccessError, ChunkLimits, ChunkStore, ChunkStoreError, Spans};
pub use writer::{SequentialWriter, WriterError};
