use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicBool, Ordering},
};

/// Single-owner static storage explicitly initialized and dropped for each probe cycle.
pub(super) struct ReusableSlot<T> {
    value: UnsafeCell<MaybeUninit<T>>,
    initialized: AtomicBool,
}

unsafe impl<T> Sync for ReusableSlot<T> {}

impl<T> ReusableSlot<T> {
    pub(super) const fn new() -> Self {
        Self {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            initialized: AtomicBool::new(false),
        }
    }

    #[allow(clippy::mut_from_ref)]
    pub(super) fn initialize(&'static self, value: T) -> &'static mut T {
        assert!(
            !self.initialized.swap(true, Ordering::AcqRel),
            "reusable BLE slot initialized twice"
        );
        unsafe { (*self.value.get()).write(value) }
    }

    /// Drop the value after every future borrowing it has been cancelled and dropped.
    pub(super) unsafe fn clear(&'static self) {
        assert!(
            self.initialized.swap(false, Ordering::AcqRel),
            "reusable BLE slot cleared while empty"
        );
        unsafe { (*self.value.get()).assume_init_drop() };
    }
}
