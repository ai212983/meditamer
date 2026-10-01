# psram-store

`no_std` bounded chunked byte store for PSRAM-backed bulk assets.

## What this crate owns

- `ChunkStore`: a logical byte length split into bounded physical chunks
  (at most `ChunkLimits::max_chunk_bytes` bytes each; only the final
  chunk may be smaller), with checked `read_exact_at` / `write_at`,
  single-chunk `slice_at`, and the `spans` iterator. Fixed-size chunk geometry
  makes offset lookup direct rather than a metadata scan per read.
- Implements `asset_source::AssetSource`: a valid in-chunk borrow is zero-copy;
  a cross-chunk range uses the caller's bounded destination.
- Caller allocator: every data chunk and the chunk metadata allocate
  through one target-supplied allocator instance (`allocator-api2`);
  the store never touches internal RAM, the global allocator, or any
  product state. Allocation is fully fallible: a failed construction
  frees every chunk it already claimed.
- Fixed logical length: set once at construction
  (`try_new_zeroed`), zero-initialized, so random `write_at` is valid
  from the start; dropping the store frees every chunk and the metadata.
- `SequentialWriter` (optional): the ordered stream contract SD loaders
  need — every write must start exactly at the cursor; `finish` requires
  the exact length, `finish_prefix` returns the received count at a
  format maximum for the codec to validate against the header.

Shared-heap allocator limits are not residency policy: the target picks
`ChunkLimits` from device measurements, and over-bound requests are
plain size errors (`TooLong`, `TooManyChunks`, `InvalidLimits`), never
admission decisions.

## When to use it

- A bulk pack needs bounded-chunk storage through a target-owned PSRAM
  allocator without hand-written chunk machinery or whole-pack
  contiguous fits.
- An SD loader needs the optional ordered-stream contract.

## When not to use it

- The caller needs admission, leases, eviction, validated formats
  (header, geometry, CRC), or screen lifetimes: pack codecs keep format
  authority and screens own the validated store lifetime.
- The caller needs a correctness contract from shared-heap free bytes
  or largest-span probes: those remain diagnostics.

## Example

```rust
use psram_store::{ChunkLimits, ChunkStore, SequentialWriter};

let limits = ChunkLimits::new(16, 8, 128);
let mut store = ChunkStore::try_new_zeroed(40, limits, psram_alloc)?;
let mut stream = SequentialWriter::new(&mut store);
stream.push(&first_read)?;
stream.push(&second_read)?;
stream.finish()?; // exact length; or finish_prefix() at a format maximum

let mut buf = [0u8; 40];
store.read_exact_at(0, &mut buf)?;
```

## Safety, lifetime, and concurrency constraints

- Chunk memory is `unsafe`-constructed from the caller allocator; borrows
  are tied to `&self` and layout never changes on access failure.
- `ChunkStore<Global>` is `Send + Sync`; other allocators keep their own
  conditional bounds via `Vec<Chunk, A>` — sharing needs an allocator
  that supports it.
- `SequentialWriter` borrows the store mutably; rejected writes never
  move the cursor or touch store bytes.

## Maturity and deferred integrations

- Implemented and host-tested: chunking, cross-chunk access, failure
  cleanup at every position, writer gap/overlap/overrun/truncation.
- Meditamer's current Mountain path uses `RuntimeBundle` from `phase-arena`.
  This store remains a reusable host-tested primitive without a live product
  consumer.
- Not provided by this crate: validated codecs, cache eviction, or DMA.
