//! Host proof for the bounded PSRAM chunk store.
//!
//! Covers the plan's reusable-store proof list: one-chunk success, a
//! shorter final chunk, cross-chunk reads and writes, offset overflow,
//! random writes, two live stores sharing one allocator, chunk-count
//! exhaustion, unrepresentable chunk limits, cleanup after failed allocation
//! at every position, and the sequential writer's gap, duplicate,
//! truncation, full-store extra-byte, and extra-data handling with store
//! bytes unchanged by rejected writes.

use allocator_api2::alloc::{AllocError, Allocator, Global, Layout};
use asset_source::AssetSource;
use core::cell::Cell;
use core::ptr::NonNull;
use psram_store::{
    AccessError, ChunkLimits, ChunkStore, ChunkStoreError, SequentialWriter, WriterError,
};

/// Failure-injecting allocator standing in for the target PSRAM region.
///
/// Counts live bytes/blocks so tests prove failed constructions release
/// every partial chunk. Each test owns its instance; no sharing.
struct TestAlloc {
    allocations_before_failure: Cell<usize>,
    live_bytes: Cell<usize>,
    live_blocks: Cell<usize>,
}

impl std::fmt::Debug for TestAlloc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestAlloc")
            .field("live_bytes", &self.live_bytes())
            .field("live_blocks", &self.live_blocks())
            .finish()
    }
}

impl TestAlloc {
    fn new() -> Self {
        Self::failing_after(usize::MAX)
    }

    fn failing_after(allocations: usize) -> Self {
        Self {
            allocations_before_failure: Cell::new(allocations),
            live_bytes: Cell::new(0),
            live_blocks: Cell::new(0),
        }
    }

    fn live_bytes(&self) -> usize {
        self.live_bytes.get()
    }

    fn live_blocks(&self) -> usize {
        self.live_blocks.get()
    }
}

// SAFETY: blocks come from the global allocator, stay valid until
// deallocated exactly once, and the allocator carries no state that
// moving it could invalidate.
unsafe impl Allocator for TestAlloc {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let remaining = self.allocations_before_failure.get();
        if remaining == 0 {
            return Err(AllocError);
        }
        self.allocations_before_failure.set(remaining - 1);
        let size = layout.size().max(1);
        let layout = Layout::from_size_align(size, layout.align()).map_err(|_| AllocError)?;
        let raw = unsafe { std::alloc::alloc(layout) };
        let ptr = NonNull::new(raw).ok_or(AllocError)?;
        self.live_bytes.set(self.live_bytes.get() + size);
        self.live_blocks.set(self.live_blocks.get() + 1);
        Ok(NonNull::slice_from_raw_parts(ptr, size))
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        let size = layout.size().max(1);
        self.live_bytes.set(self.live_bytes.get() - size);
        self.live_blocks.set(self.live_blocks.get() - 1);
        let layout = Layout::from_size_align(size, layout.align()).expect("stored layout");
        unsafe {
            std::alloc::dealloc(ptr.as_ptr(), layout);
        }
    }
}

const LIMITS: ChunkLimits = ChunkLimits::new(16, 8, 128);

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 37 + 11) as u8).collect()
}

#[test]
fn chunk_store_with_global_allocator_is_send_sync() {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ChunkStore<Global>>();
}

#[test]
fn single_chunk_store_uses_one_allocation() {
    let alloc = TestAlloc::new();
    let store = ChunkStore::try_new_zeroed(16, LIMITS, alloc).expect("one chunk");
    assert_eq!(store.len(), 16);
    assert_eq!(store.chunk_count(), 1);
    assert_eq!(store.chunk_len(0), Some(16));
    assert_eq!(store.chunk_len(1), None);
    assert_eq!(store.spans().len(), 1);
    assert!(store.slice_at(0, 16).is_some());
}

#[test]
fn final_chunk_holds_only_the_remainder() {
    let alloc = TestAlloc::new();
    let store = ChunkStore::try_new_zeroed(40, LIMITS, alloc).expect("three chunks");
    assert_eq!(store.chunk_count(), 3);
    assert_eq!(store.chunk_len(0), Some(16));
    assert_eq!(store.chunk_len(1), Some(16));
    assert_eq!(store.chunk_len(2), Some(8));
    let spans: Vec<&[u8]> = store.spans().collect();
    assert_eq!(spans.len(), 3);
    assert_eq!(spans[2].len(), 8);
}

#[test]
fn fresh_store_reads_as_zero() {
    let alloc = TestAlloc::new();
    let store = ChunkStore::try_new_zeroed(40, LIMITS, alloc).expect("store");
    let mut buf = vec![0xAA; 40];
    store.read_exact_at(0, &mut buf).expect("read");
    assert_eq!(buf, vec![0; 40]);
}

#[test]
fn writes_crossing_chunk_boundaries_round_trip() {
    let alloc = TestAlloc::new();
    let mut store = ChunkStore::try_new_zeroed(40, LIMITS, alloc).expect("store");
    let expected = pattern(40);
    // One write spanning all three chunks, then reads straddling each
    // boundary in both directions.
    store.write_at(0, &expected).expect("write");
    for (offset, len) in [
        (0, 40),
        (0, 16),
        (14, 4),
        (15, 2),
        (16, 16),
        (30, 10),
        (39, 1),
    ] {
        let mut buf = vec![0; len];
        store.read_exact_at(offset, &mut buf).expect("read");
        assert_eq!(buf, expected[offset..offset + len], "offset {offset}");
    }
}

#[test]
fn random_writes_leave_other_bytes_zeroed() {
    let alloc = TestAlloc::new();
    let mut store = ChunkStore::try_new_zeroed(48, LIMITS, alloc).expect("store");
    store.write_at(0, &[1, 2, 3]).expect("write");
    store.write_at(15, &[9, 9, 9]).expect("boundary write");
    store.write_at(47, &[7]).expect("last byte");
    let mut buf = vec![0; 48];
    store.read_exact_at(0, &mut buf).expect("read");
    let mut expected = vec![0; 48];
    expected[0..3].copy_from_slice(&[1, 2, 3]);
    expected[15..18].copy_from_slice(&[9, 9, 9]);
    expected[47] = 7;
    assert_eq!(buf, expected);
}

#[test]
fn out_of_bounds_access_fails_without_touching_layout() {
    let alloc = TestAlloc::new();
    let mut store = ChunkStore::try_new_zeroed(32, LIMITS, alloc).expect("store");
    let over = AccessError::OutOfBounds {
        offset: 31,
        len: 2,
        capacity: 32,
    };
    assert_eq!(store.read_exact_at(31, &mut [0; 2]), Err(over));
    assert_eq!(store.write_at(31, &[0; 2]), Err(over));
    assert_eq!(store.slice_at(31, 2), None);
    // Layout unchanged: full read still works and stays zeroed.
    let mut buf = vec![0xAA; 32];
    store.read_exact_at(0, &mut buf).expect("read");
    assert_eq!(buf, vec![0; 32]);
}

#[test]
fn offset_overflow_is_rejected() {
    let alloc = TestAlloc::new();
    let mut store = ChunkStore::try_new_zeroed(32, LIMITS, alloc).expect("store");
    assert_eq!(
        store.read_exact_at(usize::MAX, &mut [0; 1]),
        Err(AccessError::Overflow)
    );
    assert_eq!(store.write_at(usize::MAX, &[1]), Err(AccessError::Overflow));
    assert_eq!(store.slice_at(usize::MAX, 1), None);
}

#[test]
fn empty_ranges_at_the_end_are_no_ops() {
    let alloc = TestAlloc::new();
    let mut store = ChunkStore::try_new_zeroed(32, LIMITS, alloc).expect("store");
    store.write_at(32, &[]).expect("empty write at end");
    store.read_exact_at(32, &mut []).expect("empty read at end");
    assert_eq!(store.slice_at(32, 0), Some(&[][..]));
    assert_eq!(store.slice_at(33, 0), None);
}

#[test]
fn empty_store_holds_no_chunks() {
    let alloc = TestAlloc::new();
    let store = ChunkStore::try_new_zeroed(0, LIMITS, alloc).expect("empty");
    assert!(store.is_empty());
    assert_eq!(store.chunk_count(), 0);
    assert_eq!(store.spans().len(), 0);
    assert_eq!(store.slice_at(0, 0), Some(&[][..]));
}

#[test]
fn slice_at_only_serves_single_chunk_ranges() {
    let alloc = TestAlloc::new();
    let store = ChunkStore::try_new_zeroed(40, LIMITS, alloc).expect("store");
    assert_eq!(store.slice_at(0, 16).map(|s| s.len()), Some(16));
    assert_eq!(store.slice_at(16, 16).map(|s| s.len()), Some(16));
    assert_eq!(store.slice_at(32, 8).map(|s| s.len()), Some(8));
    assert_eq!(store.slice_at(15, 2), None);
    assert_eq!(store.slice_at(0, 40), None);
}

#[test]
fn asset_source_borrows_one_chunk_and_copies_across_boundaries() {
    let alloc = TestAlloc::new();
    let mut store = ChunkStore::try_new_zeroed(40, LIMITS, alloc).expect("store");
    store.write_at(0, &pattern(40)).expect("populate");
    assert_eq!(
        AssetSource::borrow_at(&store, 14, 2).unwrap(),
        Some(&pattern(40)[14..16])
    );
    assert_eq!(AssetSource::borrow_at(&store, 15, 2).unwrap(), None);
    let mut across = [0; 18];
    AssetSource::read_exact_at(&store, 14, &mut across).expect("cross two chunks");
    assert_eq!(&across, &pattern(40)[14..32]);
    assert_eq!(
        AssetSource::borrow_at(&store, 40, 0).unwrap(),
        Some(&[][..])
    );
    assert_eq!(
        AssetSource::borrow_at(&store, 39, 2),
        Err(AccessError::OutOfBounds {
            offset: 39,
            len: 2,
            capacity: 40
        })
    );
    assert_eq!(
        AssetSource::borrow_at(&store, usize::MAX, 2),
        Err(AccessError::Overflow)
    );
}

#[test]
fn two_live_stores_stay_independent() {
    // Both stores borrow the same target allocator instance, as on device.
    let alloc = TestAlloc::new();
    {
        let mut a = ChunkStore::try_new_zeroed(32, LIMITS, &alloc).expect("first");
        let mut b = ChunkStore::try_new_zeroed(32, LIMITS, &alloc).expect("second");
        a.write_at(0, &[1; 32]).expect("write a");
        b.write_at(0, &[2; 32]).expect("write b");
        let mut buf = vec![0; 32];
        a.read_exact_at(0, &mut buf).expect("read a");
        assert_eq!(buf, vec![1; 32]);
        b.read_exact_at(0, &mut buf).expect("read b");
        assert_eq!(buf, vec![2; 32]);
    }
    assert_eq!(alloc.live_blocks(), 0);
    assert_eq!(alloc.live_bytes(), 0);
}

#[test]
fn requests_beyond_target_bounds_are_size_errors() {
    let alloc = TestAlloc::new();
    assert_eq!(
        ChunkStore::try_new_zeroed(129, LIMITS, alloc).unwrap_err(),
        ChunkStoreError::TooLong {
            len: 129,
            max_total_bytes: 128
        }
    );
    // 128 bytes need 8 chunks of 16; cap the count at 4.
    let tight = ChunkLimits::new(16, 4, 128);
    let alloc = TestAlloc::new();
    assert_eq!(
        ChunkStore::try_new_zeroed(128, tight, alloc).unwrap_err(),
        ChunkStoreError::TooManyChunks {
            chunks: 8,
            max_chunks: 4
        }
    );
    let alloc = TestAlloc::new();
    assert_eq!(
        ChunkStore::try_new_zeroed(1, ChunkLimits::new(0, 8, 128), alloc).unwrap_err(),
        ChunkStoreError::InvalidLimits
    );
    // Exact-bound requests succeed.
    let alloc = TestAlloc::new();
    let store = ChunkStore::try_new_zeroed(64, tight, alloc).expect("at the bounds");
    assert_eq!(store.chunk_count(), 4);
}

#[test]
fn unrepresentable_chunk_size_is_invalid_limits() {
    // `usize::MAX` is not a valid `Layout` size, so it cannot back any
    // store even though it is nonzero.
    let huge = ChunkLimits::new(usize::MAX, 8, usize::MAX);
    assert_eq!(huge.chunk_count_for(1), Err(ChunkStoreError::InvalidLimits));
    assert_eq!(huge.chunk_count_for(0), Err(ChunkStoreError::InvalidLimits));
    let alloc = TestAlloc::new();
    assert_eq!(
        ChunkStore::try_new_zeroed(1, huge, alloc).unwrap_err(),
        ChunkStoreError::InvalidLimits
    );
    // No allocation was attempted: a fresh allocator stays quiet.
    let alloc = TestAlloc::failing_after(0);
    assert_eq!(
        ChunkStore::try_new_zeroed(1, huge, alloc).unwrap_err(),
        ChunkStoreError::InvalidLimits
    );
}

#[test]
fn failed_construction_releases_partial_chunks() {
    // 48 bytes need 3 data chunks plus one metadata reservation (4
    // allocations). Fail at each position and prove nothing leaks.
    // `failing_after(n)` lets `n` allocations succeed before failing.
    let expected = [
        (0, 0), // metadata reservation fails
        (1, 0), // first data chunk fails
        (2, 1), // second data chunk fails
        (3, 2), // third data chunk fails
    ];
    for (failing_after, allocated) in expected {
        let alloc = TestAlloc::failing_after(failing_after);
        // The store borrows the allocator, so live counts stay observable.
        let error = ChunkStore::try_new_zeroed(48, LIMITS, &alloc).expect_err("must fail");
        assert_eq!(
            error,
            ChunkStoreError::AllocFailed {
                allocated,
                requested: 3
            },
            "failing_after {failing_after}"
        );
        assert_eq!(alloc.live_bytes(), 0, "failing_after {failing_after}");
        assert_eq!(alloc.live_blocks(), 0, "failing_after {failing_after}");
    }
}

#[test]
fn drop_frees_data_and_metadata() {
    let alloc = TestAlloc::new();
    {
        let store = ChunkStore::try_new_zeroed(48, LIMITS, &alloc).expect("store");
        assert!(alloc.live_blocks() >= 4); // 3 data chunks + chunk metadata
        drop(store);
    }
    assert_eq!(alloc.live_blocks(), 0);
    assert_eq!(alloc.live_bytes(), 0);
}

#[test]
fn metadata_reservation_failure_allocates_nothing() {
    let alloc = TestAlloc::failing_after(0);
    let error = ChunkStore::try_new_zeroed(48, LIMITS, alloc).expect_err("must fail");
    assert_eq!(
        error,
        ChunkStoreError::AllocFailed {
            allocated: 0,
            requested: 3
        }
    );
}

#[test]
fn writer_streams_exact_length_in_order() {
    let alloc = TestAlloc::new();
    let mut store = ChunkStore::try_new_zeroed(40, LIMITS, alloc).expect("store");
    let expected = pattern(40);
    let mut writer = SequentialWriter::new(&mut store);
    writer.push(&expected[..10]).expect("push");
    assert_eq!(writer.position(), 10);
    assert_eq!(writer.remaining(), 30);
    writer.write_at(10, &expected[10..33]).expect("write_at");
    writer.push(&expected[33..]).expect("tail");
    writer.finish().expect("finish");
    let mut buf = vec![0; 40];
    store.read_exact_at(0, &mut buf).expect("read");
    assert_eq!(buf, expected);
}

fn store_bytes(store: &ChunkStore<TestAlloc>) -> Vec<u8> {
    let mut buf = vec![0xAA; store.len()];
    store.read_exact_at(0, &mut buf).expect("read");
    buf
}

fn assert_prefix_plus_zeroes(store: &ChunkStore<TestAlloc>, prefix: &[u8]) {
    let mut expected = vec![0; store.len()];
    expected[..prefix.len()].copy_from_slice(prefix);
    assert_eq!(store_bytes(store), expected);
}

#[test]
fn writer_rejects_gaps_duplicates_and_extra_data() {
    let alloc = TestAlloc::new();
    let mut store = ChunkStore::try_new_zeroed(32, LIMITS, alloc).expect("store");
    // Each rejected payload uses a value distinct from the accepted prefix
    // and from zeroes, so the read after each rejection proves it never
    // landed.
    {
        let mut writer = SequentialWriter::new(&mut store);
        writer.push(&[1; 8]).expect("push");
        assert_eq!(
            writer.write_at(16, &[9; 8]),
            Err(WriterError::Gap {
                expected: 8,
                got: 16
            })
        );
        assert_eq!(writer.position(), 8);
    }
    assert_prefix_plus_zeroes(&store, &[1; 8]);
    {
        // A fresh writer restarts at offset zero, so rewrite the identical
        // prefix to return the cursor to 8 before probing the overlap.
        let mut writer = SequentialWriter::new(&mut store);
        writer.push(&[1; 8]).expect("re-push prefix");
        assert_eq!(
            writer.write_at(4, &[2; 4]),
            Err(WriterError::Overlap {
                expected: 8,
                got: 4
            })
        );
        assert_eq!(writer.position(), 8);
    }
    assert_prefix_plus_zeroes(&store, &[1; 8]);
    {
        // Same cursor setup before probing the overrun.
        let mut writer = SequentialWriter::new(&mut store);
        writer.push(&[1; 8]).expect("re-push prefix");
        assert_eq!(
            writer.push(&[3; 25]),
            Err(WriterError::Overrun {
                offset: 8,
                len: 25,
                capacity: 32
            })
        );
        assert_eq!(writer.position(), 8);
    }
    assert_prefix_plus_zeroes(&store, &[1; 8]);
    // The unchanged store remains reusable: a fresh sequential transfer completes.
    let mut writer = SequentialWriter::new(&mut store);
    writer.push(&[1; 8]).expect("re-push prefix");
    writer.push(&[2; 24]).expect("rest");
    writer.finish().expect("finish");
    let mut expected = vec![2; 32];
    expected[..8].copy_from_slice(&[1; 8]);
    assert_eq!(store_bytes(&store), expected);
}

#[test]
fn writer_rejects_extra_byte_when_full() {
    let alloc = TestAlloc::new();
    let mut store = ChunkStore::try_new_zeroed(32, LIMITS, alloc).expect("store");
    let mut writer = SequentialWriter::new(&mut store);
    writer.push(&[4; 32]).expect("fill");
    assert_eq!(writer.position(), 32);
    assert_eq!(writer.remaining(), 0);
    assert_eq!(
        writer.push(&[5]),
        Err(WriterError::Overrun {
            offset: 32,
            len: 1,
            capacity: 32
        })
    );
    assert_eq!(writer.position(), 32);
    writer.finish().expect("finish");
    let mut buf = vec![0; 32];
    store.read_exact_at(0, &mut buf).expect("read");
    assert_eq!(buf, vec![4; 32]);
}

#[test]
fn writer_finish_requires_the_full_length() {
    let alloc = TestAlloc::new();
    let mut store = ChunkStore::try_new_zeroed(32, LIMITS, alloc).expect("store");
    let mut writer = SequentialWriter::new(&mut store);
    writer.push(&[5; 16]).expect("push");
    assert_eq!(
        writer.finish(),
        Err(WriterError::Truncated {
            received: 16,
            expected: 32
        })
    );
}

#[test]
fn writer_prefix_finish_returns_received_count() {
    let alloc = TestAlloc::new();
    let mut store = ChunkStore::try_new_zeroed(64, LIMITS, alloc).expect("store");
    let mut writer = SequentialWriter::new(&mut store);
    writer.push(&pattern(20)).expect("push");
    assert_eq!(writer.finish_prefix(), 20);
    // The codec validates the 20 bytes against the header afterwards.
    let mut buf = vec![0; 20];
    store.read_exact_at(0, &mut buf).expect("read");
    assert_eq!(buf, pattern(20));
}

#[test]
fn writer_empty_push_at_cursor_is_a_no_op() {
    let alloc = TestAlloc::new();
    let mut store = ChunkStore::try_new_zeroed(16, LIMITS, alloc).expect("store");
    let mut writer = SequentialWriter::new(&mut store);
    writer.push(&[]).expect("empty push");
    assert_eq!(writer.position(), 0);
    writer.push(&[7; 16]).expect("push");
    writer.push(&[]).expect("empty push at end");
    writer.finish().expect("finish");
}
