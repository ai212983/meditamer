//! Reusable static storage for a value whose lifecycle is externally owned.
//!
//! Initialization and destruction use distinct atomic states, preventing a
//! new value from being written while the previous value is still being
//! dropped. Teardown remains unsafe because [`ReusableSlot::initialize`]
//! returns a static mutable reference; the slot cannot observe references
//! retained by host tasks, callbacks, or values built from that reference.

use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicU8, Ordering},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum SlotState {
    Empty,
    Initializing,
    Initialized,
    Clearing,
}

impl SlotState {
    const fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Empty,
            1 => Self::Initializing,
            2 => Self::Initialized,
            3 => Self::Clearing,
            _ => unreachable!(),
        }
    }
}

pub struct ReusableSlot<T> {
    value: UnsafeCell<MaybeUninit<T>>,
    state: AtomicU8,
}

// SAFETY: all access to `value` is gated by the atomic state machine. Safe
// initialization returns `&mut T`, whose own `Send` bound controls whether a
// caller may move that reference to another executor or core. Destruction is
// unsafe and requires the caller to preserve `T`'s execution-context rules.
unsafe impl<T> Sync for ReusableSlot<T> {}

impl<T> ReusableSlot<T> {
    pub const fn new() -> Self {
        Self {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(SlotState::Empty as u8),
        }
    }

    /// Initialize the slot.
    ///
    /// Panics if the previous value has not been cleared or another lifecycle
    /// transition is in progress. The returned reference remains valid until
    /// the next call to [`Self::clear`].
    #[allow(clippy::mut_from_ref)]
    pub fn initialize(&'static self, value: T) -> &'static mut T {
        self.state
            .compare_exchange(
                SlotState::Empty as u8,
                SlotState::Initializing as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .unwrap_or_else(|state| {
                panic!(
                    "reusable BLE slot initialized while {:?}",
                    SlotState::from_raw(state)
                )
            });

        // SAFETY: the Empty -> Initializing transition gives this call
        // exclusive access to the uninitialized storage. Clearing cannot
        // begin until the state is published as Initialized below.
        let initialized = unsafe { (*self.value.get()).write(value) };
        self.state
            .store(SlotState::Initialized as u8, Ordering::Release);
        initialized
    }

    /// Drop the initialized value, making the slot reusable.
    ///
    /// Panics if the slot is empty or another lifecycle transition is in
    /// progress.
    ///
    /// # Safety
    ///
    /// The caller must have exclusive lifecycle authority for this slot and
    /// must ensure that no reference returned by [`Self::initialize`], or
    /// retained transitively by a task, callback, future, or another value,
    /// can be accessed again. Those obligations must remain true for the
    /// entire call, including while `T::drop` runs. For a non-`Send` `T`, the
    /// caller must also invoke `clear` on an execution context where dropping
    /// that value is valid.
    pub unsafe fn clear(&'static self) {
        self.state
            .compare_exchange(
                SlotState::Initialized as u8,
                SlotState::Clearing as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .unwrap_or_else(|state| {
                panic!(
                    "reusable BLE slot cleared while {:?}",
                    SlotState::from_raw(state)
                )
            });

        // SAFETY: the Initialized -> Clearing transition gives this call
        // exclusive access to a fully initialized value. The caller upholds
        // the reference-liveness requirements documented above.
        unsafe { (*self.value.get()).assume_init_drop() };
        self.state.store(SlotState::Empty as u8, Ordering::Release);
    }
}

impl<T> Default for ReusableSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;

    use self::std::{
        panic::{catch_unwind, AssertUnwindSafe},
        thread,
    };
    use core::sync::atomic::AtomicBool;

    static SLOT: ReusableSlot<u32> = ReusableSlot::new();

    #[test]
    fn initialize_then_clear_round_trips() {
        let value = SLOT.initialize(7);
        assert_eq!(*value, 7);
        unsafe { SLOT.clear() };
    }

    #[test]
    #[should_panic(expected = "initialized while Initialized")]
    fn double_initialize_without_clear_panics() {
        static DOUBLE_INIT: ReusableSlot<u32> = ReusableSlot::new();
        DOUBLE_INIT.initialize(1);
        DOUBLE_INIT.initialize(2);
    }

    #[test]
    #[should_panic(expected = "cleared while Empty")]
    fn clear_while_empty_panics() {
        static NEVER_INITIALIZED: ReusableSlot<u32> = ReusableSlot::new();
        unsafe { NEVER_INITIALIZED.clear() };
    }

    #[test]
    fn reinitialize_after_clear_succeeds() {
        static REUSED: ReusableSlot<u32> = ReusableSlot::new();
        REUSED.initialize(1);
        unsafe { REUSED.clear() };
        let value = REUSED.initialize(2);
        assert_eq!(*value, 2);
        unsafe { REUSED.clear() };
    }

    struct BlockingDrop {
        block: bool,
        started: &'static AtomicBool,
        release: &'static AtomicBool,
    }

    impl Drop for BlockingDrop {
        fn drop(&mut self) {
            if !self.block {
                return;
            }
            self.started.store(true, Ordering::Release);
            while !self.release.load(Ordering::Acquire) {
                thread::yield_now();
            }
        }
    }

    #[test]
    fn initialize_cannot_overlap_drop() {
        static CLEARING: ReusableSlot<BlockingDrop> = ReusableSlot::new();
        static DROP_STARTED: AtomicBool = AtomicBool::new(false);
        static DROP_RELEASE: AtomicBool = AtomicBool::new(false);

        let _ = CLEARING.initialize(BlockingDrop {
            block: true,
            started: &DROP_STARTED,
            release: &DROP_RELEASE,
        });
        let clearing = thread::spawn(|| unsafe { CLEARING.clear() });
        while !DROP_STARTED.load(Ordering::Acquire) {
            thread::yield_now();
        }

        let overlapping_initialize = catch_unwind(AssertUnwindSafe(|| {
            let _ = CLEARING.initialize(BlockingDrop {
                block: false,
                started: &DROP_STARTED,
                release: &DROP_RELEASE,
            });
        }));
        DROP_RELEASE.store(true, Ordering::Release);
        clearing.join().expect("clearing thread should complete");

        assert!(overlapping_initialize.is_err());
        let _ = CLEARING.initialize(BlockingDrop {
            block: false,
            started: &DROP_STARTED,
            release: &DROP_RELEASE,
        });
        unsafe { CLEARING.clear() };
    }
}
