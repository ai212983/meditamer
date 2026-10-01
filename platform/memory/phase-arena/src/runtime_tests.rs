extern crate std;

use allocator_api2::alloc::{AllocError, Allocator, Global, Layout};
use core::cell::Cell;
use core::ptr::NonNull;
use std::vec::Vec;

use crate::{AccessError, PartRequest, PartShape, RuntimeBundle, RuntimeBundleError};

struct Counts {
    attempts: Cell<usize>,
    allocations: Cell<usize>,
    deallocations: Cell<usize>,
    live_blocks: Cell<usize>,
    live_bytes: Cell<usize>,
    fail_at: usize,
}

impl Counts {
    fn new(fail_at: usize) -> Self {
        Self {
            attempts: Cell::new(0),
            allocations: Cell::new(0),
            deallocations: Cell::new(0),
            live_blocks: Cell::new(0),
            live_bytes: Cell::new(0),
            fail_at,
        }
    }

    fn balanced(&self) -> bool {
        self.allocations.get() == self.deallocations.get() && self.live_blocks.get() == 0
    }
}

struct TestAlloc<'a>(&'a Counts);

// SAFETY: blocks come from Global and are freed once; counters only observe balance.
unsafe impl Allocator for TestAlloc<'_> {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let attempt = self.0.attempts.get() + 1;
        self.0.attempts.set(attempt);
        if attempt == self.0.fail_at {
            return Err(AllocError);
        }
        let memory = Global.allocate(layout)?;
        self.0.allocations.set(self.0.allocations.get() + 1);
        self.0.live_blocks.set(self.0.live_blocks.get() + 1);
        self.0
            .live_bytes
            .set(self.0.live_bytes.get() + memory.len());
        Ok(memory)
    }

    fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let attempt = self.0.attempts.get() + 1;
        self.0.attempts.set(attempt);
        if attempt == self.0.fail_at {
            return Err(AllocError);
        }
        let memory = Global.allocate_zeroed(layout)?;
        self.0.allocations.set(self.0.allocations.get() + 1);
        self.0.live_blocks.set(self.0.live_blocks.get() + 1);
        self.0
            .live_bytes
            .set(self.0.live_bytes.get() + memory.len());
        Ok(memory)
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        self.0.deallocations.set(self.0.deallocations.get() + 1);
        self.0.live_blocks.set(self.0.live_blocks.get() - 1);
        self.0
            .live_bytes
            .set(self.0.live_bytes.get() - layout.size());
        // SAFETY: pointer and layout came from Global above, freed once here.
        unsafe { Global.deallocate(ptr, layout) };
    }
}

fn contiguous(bytes: usize, align: usize) -> PartRequest {
    PartRequest {
        bytes,
        align,
        shape: PartShape::Contiguous,
    }
}

fn segmented(bytes: usize, align: usize, unit: usize) -> PartRequest {
    PartRequest {
        bytes,
        align,
        shape: PartShape::Segmented {
            max_segment_bytes: unit,
        },
    }
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 37 + 11) as u8).collect()
}

#[test]
fn empty_request_is_rejected_before_allocation() {
    let counts = Counts::new(usize::MAX);
    assert_eq!(
        RuntimeBundle::try_new_zeroed(&[], 1024, TestAlloc(&counts)).unwrap_err(),
        RuntimeBundleError::Empty
    );
    assert_eq!(counts.attempts.get(), 0);
}

#[test]
fn zero_sizes_are_rejected_before_allocation() {
    let counts = Counts::new(usize::MAX);
    for bad in [
        contiguous(0, 1),
        segmented(0, 1, 16),
        segmented(8, 1, 0),
        segmented(0, 4, 0),
    ] {
        assert_eq!(
            RuntimeBundle::try_new_zeroed(&[bad], 1024, TestAlloc(&counts)).unwrap_err(),
            RuntimeBundleError::InvalidSize
        );
    }
    assert_eq!(counts.attempts.get(), 0);
}

#[test]
fn invalid_alignment_is_rejected_before_allocation() {
    let counts = Counts::new(usize::MAX);
    for bad in [
        contiguous(8, 0),
        contiguous(8, 3),
        segmented(32, 0, 16),
        segmented(32, 24, 16),
    ] {
        assert_eq!(
            RuntimeBundle::try_new_zeroed(&[bad], 1024, TestAlloc(&counts)).unwrap_err(),
            RuntimeBundleError::InvalidAlignment
        );
    }
    assert_eq!(counts.attempts.get(), 0);
}

#[test]
fn over_bound_is_rejected_before_allocation_and_exact_bound_fits() {
    let requests = [contiguous(16, 8), segmented(24, 4, 8)];
    let counts = Counts::new(usize::MAX);
    assert_eq!(
        RuntimeBundle::try_new_zeroed(&requests, 39, TestAlloc(&counts)).unwrap_err(),
        RuntimeBundleError::OverBound {
            requested: 40,
            bound: 39
        }
    );
    assert_eq!(counts.attempts.get(), 0);
    let counts = Counts::new(usize::MAX);
    let bundle = RuntimeBundle::try_new_zeroed(&requests, 40, TestAlloc(&counts)).unwrap();
    assert_eq!(bundle.part_count(), 2);
    assert_eq!(bundle.total_bytes(), 40);
    assert!(counts.attempts.get() > 0);
}

#[test]
fn total_overflow_is_rejected_before_allocation() {
    let requests = [contiguous(usize::MAX, 1), contiguous(1, 1)];
    let counts = Counts::new(usize::MAX);
    assert_eq!(
        RuntimeBundle::try_new_zeroed(&requests, usize::MAX, TestAlloc(&counts)).unwrap_err(),
        RuntimeBundleError::Overflow
    );
    assert_eq!(counts.attempts.get(), 0);
}

#[test]
fn unrepresentable_layout_frees_metadata() {
    let requests = [contiguous(usize::MAX - 1, 2)];
    let counts = Counts::new(usize::MAX);
    assert_eq!(
        RuntimeBundle::try_new_zeroed(&requests, usize::MAX, TestAlloc(&counts)).unwrap_err(),
        RuntimeBundleError::InvalidPartLayout { index: 0 }
    );
    assert!(counts.balanced());
}

#[test]
fn mixed_shapes_share_one_owner_with_shape_specific_borrows() {
    let counts = Counts::new(usize::MAX);
    let requests = [contiguous(16, 8), segmented(40, 4, 16)];
    let mut bundle = RuntimeBundle::try_new_zeroed(&requests, 64, TestAlloc(&counts)).unwrap();
    assert_eq!(bundle.part_len(0), Some(16));
    assert_eq!(bundle.part_len(1), Some(40));
    assert_eq!(bundle.is_segmented(0), Some(false));
    assert_eq!(bundle.is_segmented(1), Some(true));
    assert_eq!(bundle.part_len(2), None);
    bundle.part_mut(0).unwrap().copy_from_slice(&[7; 16]);
    bundle.write_at(1, 0, &pattern(40)).unwrap();
    assert_eq!(bundle.part(0).unwrap(), &[7; 16]);
    assert_eq!(bundle.part(1), None);
    assert_eq!(bundle.segment_at(0, 0, 16), None);
    let mut back = [0; 40];
    bundle.read_at(1, 0, &mut back).unwrap();
    assert_eq!(back, pattern(40)[..]);
    let mut edge = [0; 18];
    bundle.read_at(1, 14, &mut edge).unwrap();
    assert_eq!(edge, pattern(40)[14..32]);
}

#[test]
fn alignment_holds_for_contiguous_and_every_segment() {
    let counts = Counts::new(usize::MAX);
    let requests = [contiguous(24, 64), segmented(40, 32, 16)];
    let bundle = RuntimeBundle::try_new_zeroed(&requests, 64, TestAlloc(&counts)).unwrap();
    assert_eq!(bundle.part(0).unwrap().as_ptr() as usize % 64, 0);
    assert_eq!(bundle.segment_count(1), Some(3));
    for (offset, len) in [(0, 16), (16, 16), (32, 8)] {
        let segment = bundle.segment_at(1, offset, len).unwrap();
        assert_eq!(segment.as_ptr() as usize % 32, 0);
        assert_eq!(segment.len(), len);
    }
}

#[test]
fn offset_overflow_and_oob_leave_bytes_untouched() {
    let counts = Counts::new(usize::MAX);
    let requests = [segmented(32, 4, 8)];
    let mut bundle = RuntimeBundle::try_new_zeroed(&requests, 32, TestAlloc(&counts)).unwrap();
    bundle.write_at(0, 0, &pattern(32)).unwrap();
    assert_eq!(
        bundle.read_at(0, usize::MAX, &mut [0; 1]),
        Err(AccessError::Overflow)
    );
    assert_eq!(
        bundle.write_at(0, usize::MAX, &[1]),
        Err(AccessError::Overflow)
    );
    let over = AccessError::OutOfBounds {
        offset: 31,
        len: 2,
        capacity: 32,
    };
    assert_eq!(bundle.read_at(0, 31, &mut [0; 2]), Err(over));
    assert_eq!(bundle.write_at(0, 31, &[0; 2]), Err(over));
    assert_eq!(
        bundle.read_at(9, 0, &mut [0; 1]),
        Err(AccessError::OutOfBounds {
            offset: 0,
            len: 1,
            capacity: 0
        })
    );
    let mut back = [0xAA; 32];
    bundle.read_at(0, 0, &mut back).unwrap();
    assert_eq!(back, pattern(32)[..]);
}

#[test]
fn partial_failure_at_each_position_frees_everything() {
    let requests = [contiguous(16, 8), segmented(40, 4, 16)];
    let expected = [
        (1, None),
        (2, Some(0)),
        (3, Some(1)),
        (4, Some(1)),
        (5, Some(1)),
    ];
    for (fail_at, part) in expected {
        let counts = Counts::new(fail_at);
        let error = RuntimeBundle::try_new_zeroed(&requests, 64, TestAlloc(&counts)).unwrap_err();
        match part {
            None => assert_eq!(error, RuntimeBundleError::MetadataAllocationFailed),
            Some(index) => assert_eq!(error, RuntimeBundleError::PartAllocationFailed { index }),
        }
        assert!(counts.balanced(), "fail_at {fail_at}");
    }
    let counts = Counts::new(usize::MAX);
    let bundle = RuntimeBundle::try_new_zeroed(&requests, 64, TestAlloc(&counts)).unwrap();
    let mut zero = [0xAA; 40];
    bundle.read_at(1, 0, &mut zero).unwrap();
    assert_eq!(zero, [0; 40]);
}

#[test]
fn ownership_moves_across_threads_with_global_allocator() {
    const fn assert_send<T: Send>() {}
    assert_send::<RuntimeBundle<Global>>();
    let requests = [segmented(32, 4, 8), contiguous(8, 8)];
    let bundle = RuntimeBundle::try_new_zeroed(&requests, 40, Global).unwrap();
    let back = std::thread::spawn(move || {
        let mut bundle = bundle;
        bundle.write_at(0, 0, &pattern(32)).unwrap();
        bundle.part_mut(1).unwrap().copy_from_slice(&[3; 8]);
        let mut buf = [0; 32];
        bundle.read_at(0, 0, &mut buf).unwrap();
        assert_eq!(buf, pattern(32)[..]);
        bundle
    })
    .join()
    .unwrap();
    let mut buf = [0; 32];
    back.read_at(0, 0, &mut buf).unwrap();
    assert_eq!(buf, pattern(32)[..]);
    assert_eq!(back.part(1).unwrap(), &[3; 8]);
}

#[test]
fn drop_frees_all_and_reentry_starts_zeroed() {
    let counts = Counts::new(usize::MAX);
    let requests = [contiguous(16, 8), segmented(40, 4, 16)];
    {
        let mut bundle = RuntimeBundle::try_new_zeroed(&requests, 64, TestAlloc(&counts)).unwrap();
        bundle.part_mut(0).unwrap().copy_from_slice(&[1; 16]);
        bundle.write_at(1, 0, &[2; 40]).unwrap();
    }
    assert!(counts.balanced());
    {
        let bundle = RuntimeBundle::try_new_zeroed(&requests, 64, TestAlloc(&counts)).unwrap();
        assert_eq!(bundle.part(0).unwrap(), &[0; 16]);
        let mut back = [0xAA; 40];
        bundle.read_at(1, 0, &mut back).unwrap();
        assert_eq!(back, [0; 40]);
    }
    assert!(counts.balanced());
}

#[test]
fn segmented_access_never_coalesces_the_whole_part() {
    let counts = Counts::new(usize::MAX);
    let requests = [segmented(40, 4, 16)];
    let mut bundle = RuntimeBundle::try_new_zeroed(&requests, 40, TestAlloc(&counts)).unwrap();
    assert_eq!(bundle.segment_count(0), Some(3));
    assert_eq!(bundle.segment_len(0, 0), Some(16));
    assert_eq!(bundle.segment_len(0, 1), Some(16));
    assert_eq!(bundle.segment_len(0, 2), Some(8));
    assert_eq!(bundle.segment_len(0, 3), None);
    assert_eq!(bundle.part(0), None);
    assert_eq!(bundle.segment_at(0, 15, 2), None);
    assert_eq!(bundle.segment_at(0, 0, 40), None);
    assert_eq!(bundle.segment_at(0, 40, 1), None);
    assert_eq!(bundle.segment_at(0, 40, 0), Some(&[][..]));
    bundle.write_at(0, 0, &pattern(40)).unwrap();
    assert_eq!(bundle.segment_at(0, 0, 16).unwrap(), &pattern(40)[..16]);
    assert_eq!(bundle.segment_at(0, 16, 16).unwrap(), &pattern(40)[16..32]);
    assert_eq!(bundle.segment_at(0, 32, 8).unwrap(), &pattern(40)[32..]);
    bundle
        .segment_at_mut(0, 32, 8)
        .unwrap()
        .copy_from_slice(&[9; 8]);
    let mut back = [0; 40];
    bundle.read_at(0, 0, &mut back).unwrap();
    assert_eq!(&back[..32], &pattern(40)[..32]);
    assert_eq!(&back[32..], &[9; 8]);
}

#[test]
fn immutable_view_reads_without_taking_ownership() {
    let counts = Counts::new(usize::MAX);
    let requests = [contiguous(8, 8), segmented(24, 4, 8)];
    let mut bundle = RuntimeBundle::try_new_zeroed(&requests, 32, TestAlloc(&counts)).unwrap();
    bundle.part_mut(0).unwrap().copy_from_slice(&[4; 8]);
    bundle.write_at(1, 0, &pattern(24)).unwrap();
    {
        let view = bundle.view();
        assert_eq!(view.part_count(), 2);
        assert_eq!(view.total_bytes(), 32);
        assert_eq!(view.part_len(0), Some(8));
        assert_eq!(view.is_segmented(1), Some(true));
        assert_eq!(view.part(0).unwrap(), &[4; 8]);
        assert_eq!(view.part(1), None);
        let mut buf = [0; 24];
        view.read_at(1, 0, &mut buf).unwrap();
        assert_eq!(buf, pattern(24)[..]);
        assert_eq!(view.segment_at(1, 8, 8).unwrap(), &pattern(24)[8..16]);
        assert_eq!(view.segment_count(1), Some(3));
        assert_eq!(view.segment_len(1, 2), Some(8));
    }
    bundle.part_mut(0).unwrap().copy_from_slice(&[6; 8]);
    assert_eq!(bundle.view().part(0).unwrap(), &[6; 8]);
}
