use core::cell::UnsafeCell;

use esp_sync::RawMutex;

pub(crate) struct Locked<T> {
    lock_state: RawMutex,
    data: UnsafeCell<T>,
}

impl<T> Locked<T> {
    pub(crate) const fn new(data: T) -> Self {
        Self {
            lock_state: RawMutex::new(),
            data: UnsafeCell::new(data),
        }
    }

    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        self.lock_state
            .lock_non_reentrant(|| f(unsafe { &mut *self.data.get() }))
    }
}

unsafe impl<T> Sync for Locked<T> {}
