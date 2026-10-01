use allocator_api2::{alloc::Allocator, boxed::Box};
use core::mem::MaybeUninit;

#[inline(always)]
pub(super) fn write<T, A, Make>(mut slot: Box<MaybeUninit<T>, A>, make: Make) -> Box<T, A>
where
    A: Allocator,
    Make: FnOnce() -> T,
{
    slot.write(make());
    // SAFETY: this function writes the slot exactly once before changing its
    // initialized type. The allocator-typed box retains ownership throughout.
    unsafe { slot.assume_init() }
}
