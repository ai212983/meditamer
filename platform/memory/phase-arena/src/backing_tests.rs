extern crate std;

use allocator_api2::alloc::{AllocError, Allocator, Global, Layout};
use core::cell::Cell;
use core::ptr::NonNull;
use residency_policy::{Bundle, BundleId, BundlePart, ResourceId};

use crate::{
    prepare_backed_bundle, ArenaId, BackingError, BundleAdmissionError, BundleBacking, PhaseArena,
    PrepareBackingError,
};

const PARTS: [BundlePart; 2] = [
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
const BUNDLE: Bundle<'static> = Bundle {
    id: BundleId(1),
    version: 1,
    parts: &PARTS,
};

struct Counts {
    attempts: Cell<usize>,
    allocations: Cell<usize>,
    deallocations: Cell<usize>,
    fail_at: usize,
}

impl Counts {
    fn new(fail_at: usize) -> Self {
        Self {
            attempts: Cell::new(0),
            allocations: Cell::new(0),
            deallocations: Cell::new(0),
            fail_at,
        }
    }
}

struct TestAllocator<'a>(&'a Counts);

// SAFETY: this wrapper forwards allocations and deallocations with the same
// layouts to Global; it only injects a failure before an allocation begins.
unsafe impl Allocator for TestAllocator<'_> {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let attempt = self.0.attempts.get() + 1;
        self.0.attempts.set(attempt);
        if attempt == self.0.fail_at {
            return Err(AllocError);
        }
        let memory = Global.allocate(layout)?;
        self.0.allocations.set(self.0.allocations.get() + 1);
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
        Ok(memory)
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        self.0.deallocations.set(self.0.deallocations.get() + 1);
        // SAFETY: the pointer and layout came from Global above and are
        // deallocated once through the same test wrapper.
        unsafe { Global.deallocate(ptr, layout) };
    }
}

#[test]
fn physical_bytes_are_zeroed_aligned_and_released_for_reentry() {
    let counts = Counts::new(0);
    let mut arena = PhaseArena::new(ArenaId(1), 8, 4).unwrap();
    for _ in 0..3 {
        let mut prepared =
            prepare_backed_bundle::<_, 2>(&mut arena, &BUNDLE, TestAllocator(&counts)).unwrap();
        assert_eq!(prepared.part_mut(0), Some(&mut [0, 0, 0][..]));
        assert_eq!(prepared.part_mut(1), Some(&mut [0, 0, 0, 0][..]));
        assert_eq!((prepared.part_mut(1).unwrap().as_ptr() as usize) % 4, 0);
        prepared.part_mut(0).unwrap().copy_from_slice(&[1, 2, 3]);
        let active = prepared.commit();
        assert_eq!(active.part(0), Some(&[1, 2, 3][..]));
        active.release();
        assert_eq!(arena.requested(), 0);
    }
    assert_eq!(counts.allocations.get(), 9); // metadata plus two parts, each entry
    assert_eq!(counts.deallocations.get(), counts.allocations.get());
}

#[test]
fn failed_part_frees_preceding_bytes_and_rolls_back_arena() {
    let counts = Counts::new(3); // metadata and part 0 succeed; part 1 fails
    let mut arena = PhaseArena::new(ArenaId(1), 8, 4).unwrap();
    let result = prepare_backed_bundle::<_, 2>(&mut arena, &BUNDLE, TestAllocator(&counts));
    assert!(matches!(
        result,
        Err(PrepareBackingError::Backing(
            BackingError::PartAllocationFailed { index: 1 }
        ))
    ));
    drop(result);
    assert_eq!(arena.requested(), 0);
    assert_eq!(counts.allocations.get(), 2);
    assert_eq!(counts.deallocations.get(), 2);
}

#[test]
fn failed_metadata_and_abandoned_prepare_change_no_ownership() {
    let counts = Counts::new(1);
    let mut arena = PhaseArena::new(ArenaId(1), 8, 4).unwrap();
    assert!(matches!(
        prepare_backed_bundle::<_, 2>(&mut arena, &BUNDLE, TestAllocator(&counts)),
        Err(PrepareBackingError::Backing(
            BackingError::MetadataAllocationFailed
        ))
    ));
    assert_eq!(arena.requested(), 0);
    assert_eq!(counts.allocations.get(), 0);

    let counts = Counts::new(0);
    let prepared =
        prepare_backed_bundle::<_, 2>(&mut arena, &BUNDLE, TestAllocator(&counts)).unwrap();
    drop(prepared);
    assert_eq!(arena.requested(), 0);
    assert_eq!(counts.allocations.get(), counts.deallocations.get());
}

#[test]
fn structural_rejection_performs_no_physical_allocation() {
    let counts = Counts::new(0);
    let mut arena = PhaseArena::new(ArenaId(1), 7, 4).unwrap();
    assert!(matches!(
        prepare_backed_bundle::<_, 2>(&mut arena, &BUNDLE, TestAllocator(&counts)),
        Err(PrepareBackingError::Arena(
            BundleAdmissionError::InsufficientCapacity {
                required: 8,
                available: 7
            }
        ))
    ));
    assert_eq!(arena.requested(), 0);
    assert_eq!(counts.attempts.get(), 0);
}

#[test]
fn dropping_active_bundle_releases_physical_bytes_and_arena() {
    let counts = Counts::new(0);
    let mut origin = PhaseArena::new(ArenaId(1), 8, 4).unwrap();
    let mut active = prepare_backed_bundle::<_, 2>(&mut origin, &BUNDLE, TestAllocator(&counts))
        .unwrap()
        .commit();
    active.part_mut(0).unwrap().copy_from_slice(&[4, 5, 6]);
    assert_eq!(active.part(0), Some(&[4, 5, 6][..]));
    assert_eq!(counts.deallocations.get(), 0);
    drop(active);
    assert_eq!(origin.requested(), 0);
    assert_eq!(counts.allocations.get(), counts.deallocations.get());
}

#[test]
fn standalone_backing_accepts_non_product_layouts() {
    let counts = Counts::new(0);
    let parts = [BundlePart {
        resource: ResourceId(91),
        bytes: 17,
        align: 64,
    }];
    let bundle = Bundle {
        id: BundleId(91),
        version: 2,
        parts: &parts,
    };
    let mut backing = BundleBacking::try_new_zeroed(&bundle, 64, TestAllocator(&counts)).unwrap();
    assert_eq!(backing.id(), BundleId(91));
    assert_eq!(backing.version(), 2);
    assert_eq!(backing.part_count(), 1);
    assert_eq!((backing.part_mut(0).unwrap().as_ptr() as usize) % 64, 0);
    assert_eq!(backing.part(0).unwrap(), &[0; 17]);
    drop(backing);
    assert_eq!(counts.allocations.get(), counts.deallocations.get());
}
