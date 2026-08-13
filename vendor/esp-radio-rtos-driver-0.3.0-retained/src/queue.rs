//! # Queues
//!
//! Queues are a synchronization primitive used to communicate between tasks.
//! They allow tasks to send and receive data in a first-in-first-out (FIFO) manner.
//!
//! ## Implementation
//!
//! Implement the `QueueImplementation` trait for an object, and use the
//! `register_queue_implementation` to register that implementation for esp-radio.
//!
//! See the [`QueueImplementation`] documentation for more information.
//!
//! You may also choose to use the [`CompatQueue`] implementation provided by this crate.
//!
//! ## Usage
//!
//! Users should use [`QueueHandle`] to interact with queues created by the driver implementation.
//!
//! > Note that the only expected user of this crate is esp-radio.

use core::ptr::NonNull;

/// Pointer to an opaque queue created by the driver implementation.
pub type QueuePtr = NonNull<()>;

unsafe extern "Rust" {
    fn esp_rtos_queue_create(capacity: usize, item_size: usize) -> QueuePtr;
    fn esp_rtos_queue_delete(queue: QueuePtr);

    fn esp_rtos_queue_send_to_front(
        queue: QueuePtr,
        item: *const u8,
        timeout_us: Option<u32>,
    ) -> bool;
    fn esp_rtos_queue_send_to_front_with_deadline(
        queue: QueuePtr,
        item: *const u8,
        deadline_instant: Option<u64>,
    ) -> bool;

    fn esp_rtos_queue_send_to_back(
        queue: QueuePtr,
        item: *const u8,
        timeout_us: Option<u32>,
    ) -> bool;
    fn esp_rtos_queue_send_to_back_with_deadline(
        queue: QueuePtr,
        item: *const u8,
        deadline_instant: Option<u64>,
    ) -> bool;

    fn esp_rtos_queue_try_send_to_back_from_isr(
        queue: QueuePtr,
        item: *const u8,
        higher_prio_task_waken: Option<&mut bool>,
    ) -> bool;
    fn esp_rtos_queue_receive(queue: QueuePtr, item: *mut u8, timeout_us: Option<u32>) -> bool;
    fn esp_rtos_queue_receive_with_deadline(
        queue: QueuePtr,
        item: *mut u8,
        deadline_instant: Option<u64>,
    ) -> bool;
    fn esp_rtos_queue_try_receive_from_isr(
        queue: QueuePtr,
        item: *mut u8,
        higher_prio_task_waken: Option<&mut bool>,
    ) -> bool;
    fn esp_rtos_queue_remove(queue: QueuePtr, item: *const u8);
    fn esp_rtos_queue_messages_waiting(queue: QueuePtr) -> usize;
}

/// A queue primitive.
///
/// The following snippet demonstrates the boilerplate necessary to implement a queue using the
/// `QueueImplementation` trait:
///
/// ```rust,no_run
/// use esp_radio_rtos_driver::{
///     queue::{QueueImplementation, QueuePtr},
///     register_queue_implementation,
/// };
///
/// struct MyQueue {
///     // Queue implementation details
/// }
///
/// impl QueueImplementation for MyQueue {
///     fn create(capacity: usize, item_size: usize) -> QueuePtr {
///         unimplemented!()
///     }
///
///     unsafe fn delete(queue: QueuePtr) {
///         unimplemented!()
///     }
///
///     unsafe fn send_to_front(queue: QueuePtr, item: *const u8, timeout_us: Option<u32>) -> bool {
///         unimplemented!()
///     }
///
///     unsafe fn send_to_back(queue: QueuePtr, item: *const u8, timeout_us: Option<u32>) -> bool {
///         unimplemented!()
///     }
///
///     unsafe fn try_send_to_back_from_isr(
///         queue: QueuePtr,
///         item: *const u8,
///         higher_prio_task_waken: Option<&mut bool>,
///     ) -> bool {
///         unimplemented!()
///     }
///
///     unsafe fn receive(queue: QueuePtr, item: *mut u8, timeout_us: Option<u32>) -> bool {
///         unimplemented!()
///     }
///
///     unsafe fn try_receive_from_isr(
///         queue: QueuePtr,
///         item: *mut u8,
///         higher_prio_task_waken: Option<&mut bool>,
///     ) -> bool {
///         unimplemented!()
///     }
///
///     unsafe fn remove(queue: QueuePtr, item: *const u8) {
///         unimplemented!()
///     }
///
///     fn messages_waiting(queue: QueuePtr) -> usize {
///         unimplemented!()
///     }
/// }
///
/// register_queue_implementation!(MyQueue);
/// ```
pub trait QueueImplementation {
    /// Creates a new, empty queue instance.
    ///
    /// The queue must have a capacity for `capacity` number of `item_size` byte items.
    fn create(capacity: usize, item_size: usize) -> QueuePtr;

    /// Deletes a queue instance.
    ///
    /// # Safety
    ///
    /// `queue` must be a pointer returned from [`Self::create`].
    unsafe fn delete(queue: QueuePtr);

    /// Enqueues a high-priority item.
    ///
    /// If the queue is full, this function will block for the given timeout. If timeout is None,
    /// the function will block indefinitely.
    ///
    /// This function returns `true` if the item was successfully enqueued, `false` otherwise.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    unsafe fn send_to_front(queue: QueuePtr, item: *const u8, timeout_us: Option<u32>) -> bool;

    /// Enqueues a high-priority item.
    ///
    /// If the queue is full, this function will block until the deadline is reached. If the
    /// deadline is None, the function will block indefinitely.
    ///
    /// This function returns `true` if the item was successfully enqueued, `false` otherwise.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    unsafe fn send_to_front_with_deadline(
        queue: QueuePtr,
        item: *const u8,
        deadline_instant: Option<u64>,
    ) -> bool;

    /// Enqueues an item.
    ///
    /// If the queue is full, this function will block for the given timeout. If timeout is None,
    /// the function will block indefinitely.
    ///
    /// This function returns `true` if the item was successfully enqueued, `false` otherwise.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    unsafe fn send_to_back(queue: QueuePtr, item: *const u8, timeout_us: Option<u32>) -> bool;

    /// Enqueues an item.
    ///
    /// If the queue is full, this function will block until the given deadline. If deadline is
    /// None, the function will block indefinitely.
    ///
    /// This function returns `true` if the item was successfully enqueued, `false` otherwise.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    unsafe fn send_to_back_with_deadline(
        queue: QueuePtr,
        item: *const u8,
        deadline_instant: Option<u64>,
    ) -> bool;

    /// Attempts to enqueues an item.
    ///
    /// If the queue is full, this function will immediately return `false`.
    ///
    /// The `higher_prio_task_waken` parameter is an optional mutable reference to a boolean flag.
    /// If the flag is `Some`, the implementation may set it to `true` to request a context switch.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    unsafe fn try_send_to_back_from_isr(
        queue: QueuePtr,
        item: *const u8,
        higher_prio_task_waken: Option<&mut bool>,
    ) -> bool;

    /// Dequeues an item from the queue.
    ///
    /// If the queue is empty, this function will block for the given timeout. If timeout is None,
    /// the function will block indefinitely.
    ///
    /// This function returns `true` if the item was successfully dequeued, `false` otherwise.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    unsafe fn receive(queue: QueuePtr, item: *mut u8, timeout_us: Option<u32>) -> bool;

    /// Dequeues an item from the queue.
    ///
    /// If the queue is empty, this function will block until the given deadline is reached. If the
    /// deadline is None, the function will block indefinitely.
    ///
    /// This function returns `true` if the item was successfully dequeued, `false` otherwise.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    unsafe fn receive_with_deadline(
        queue: QueuePtr,
        item: *mut u8,
        deadline_instant: Option<u64>,
    ) -> bool;

    /// Attempts to dequeue an item from the queue.
    ///
    /// If the queue is empty, this function will return `false` immediately.
    ///
    /// The `higher_prio_task_waken` parameter is an optional mutable reference to a boolean flag.
    /// If the flag is `Some`, the implementation may set it to `true` to request a context switch.
    ///
    /// This function returns `true` if the item was successfully dequeued, `false` otherwise.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    unsafe fn try_receive_from_isr(
        queue: QueuePtr,
        item: *mut u8,
        higher_prio_task_waken: Option<&mut bool>,
    ) -> bool;

    /// Removes an item from the queue.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    unsafe fn remove(queue: QueuePtr, item: *const u8);

    /// Returns the number of messages in the queue.
    fn messages_waiting(queue: QueuePtr) -> usize;
}

#[macro_export]
macro_rules! register_queue_implementation {
    ($t: ty) => {
        #[unsafe(no_mangle)]
        #[inline]
        fn esp_rtos_queue_create(capacity: usize, item_size: usize) -> $crate::queue::QueuePtr {
            <$t as $crate::queue::QueueImplementation>::create(capacity, item_size)
        }

        #[unsafe(no_mangle)]
        #[inline]
        fn esp_rtos_queue_delete(queue: $crate::queue::QueuePtr) {
            unsafe { <$t as $crate::queue::QueueImplementation>::delete(queue) }
        }

        #[unsafe(no_mangle)]
        #[inline]
        fn esp_rtos_queue_send_to_front(
            queue: $crate::queue::QueuePtr,
            item: *const u8,
            timeout_us: Option<u32>,
        ) -> bool {
            unsafe {
                <$t as $crate::queue::QueueImplementation>::send_to_front(queue, item, timeout_us)
            }
        }

        #[unsafe(no_mangle)]
        #[inline]
        fn esp_rtos_queue_send_to_front_with_deadline(
            queue: $crate::queue::QueuePtr,
            item: *const u8,
            deadline_instant: Option<u64>,
        ) -> bool {
            unsafe {
                <$t as $crate::queue::QueueImplementation>::send_to_front_with_deadline(
                    queue,
                    item,
                    deadline_instant,
                )
            }
        }

        #[unsafe(no_mangle)]
        #[inline]
        fn esp_rtos_queue_send_to_back(
            queue: $crate::queue::QueuePtr,
            item: *const u8,
            timeout_us: Option<u32>,
        ) -> bool {
            unsafe {
                <$t as $crate::queue::QueueImplementation>::send_to_back(queue, item, timeout_us)
            }
        }

        #[unsafe(no_mangle)]
        #[inline]
        fn esp_rtos_queue_send_to_back_with_deadline(
            queue: $crate::queue::QueuePtr,
            item: *const u8,
            deadline_instant: Option<u64>,
        ) -> bool {
            unsafe {
                <$t as $crate::queue::QueueImplementation>::send_to_back_with_deadline(
                    queue,
                    item,
                    deadline_instant,
                )
            }
        }

        #[unsafe(no_mangle)]
        #[inline]
        fn esp_rtos_queue_try_send_to_back_from_isr(
            queue: $crate::queue::QueuePtr,
            item: *const u8,
            higher_prio_task_waken: Option<&mut bool>,
        ) -> bool {
            unsafe {
                <$t as $crate::queue::QueueImplementation>::try_send_to_back_from_isr(
                    queue,
                    item,
                    higher_prio_task_waken,
                )
            }
        }

        #[unsafe(no_mangle)]
        #[inline]
        fn esp_rtos_queue_receive(
            queue: $crate::queue::QueuePtr,
            item: *mut u8,
            timeout_us: Option<u32>,
        ) -> bool {
            unsafe { <$t as $crate::queue::QueueImplementation>::receive(queue, item, timeout_us) }
        }

        #[unsafe(no_mangle)]
        #[inline]
        fn esp_rtos_queue_receive_with_deadline(
            queue: $crate::queue::QueuePtr,
            item: *mut u8,
            deadline_instant: Option<u64>,
        ) -> bool {
            unsafe {
                <$t as $crate::queue::QueueImplementation>::receive_with_deadline(
                    queue,
                    item,
                    deadline_instant,
                )
            }
        }

        #[unsafe(no_mangle)]
        #[inline]
        fn esp_rtos_queue_try_receive_from_isr(
            queue: $crate::queue::QueuePtr,
            item: *mut u8,
            higher_prio_task_waken: Option<&mut bool>,
        ) -> bool {
            unsafe {
                <$t as $crate::queue::QueueImplementation>::try_receive_from_isr(
                    queue,
                    item,
                    higher_prio_task_waken,
                )
            }
        }

        #[unsafe(no_mangle)]
        #[inline]
        fn esp_rtos_queue_remove(queue: $crate::queue::QueuePtr, item: *mut u8) {
            unsafe { <$t as $crate::queue::QueueImplementation>::remove(queue, item) }
        }

        #[unsafe(no_mangle)]
        #[inline]
        fn esp_rtos_queue_messages_waiting(queue: $crate::queue::QueuePtr) -> usize {
            unsafe { <$t as $crate::queue::QueueImplementation>::messages_waiting(queue) }
        }
    };
}

/// Queue handle.
///
/// This handle is used to interact with queues created by the driver implementation.
#[repr(transparent)]
pub struct QueueHandle(QueuePtr);
impl QueueHandle {
    /// Creates a new queue instance.
    #[inline]
    pub fn new(capacity: usize, item_size: usize) -> Self {
        let ptr = unsafe { esp_rtos_queue_create(capacity, item_size) };
        Self(ptr)
    }

    /// Converts this object into a pointer without dropping it.
    #[inline]
    pub fn leak(self) -> QueuePtr {
        let ptr = self.0;
        core::mem::forget(self);
        ptr
    }

    /// Recovers the object from a leaked pointer.
    ///
    /// # Safety
    ///
    /// - The caller must only use pointers created using [`Self::leak`].
    /// - The caller must ensure the pointer is not shared.
    #[inline]
    pub unsafe fn from_ptr(ptr: QueuePtr) -> Self {
        Self(ptr)
    }

    /// Creates a reference to this object from a leaked pointer.
    ///
    /// This function is used in the esp-radio code to interact with the queue.
    ///
    /// # Safety
    ///
    /// - The caller must only use pointers created using [`Self::leak`].
    #[inline]
    pub unsafe fn ref_from_ptr(ptr: &QueuePtr) -> &Self {
        unsafe { core::mem::transmute(ptr) }
    }

    /// Enqueues a high-priority item.
    ///
    /// If the queue is full, this function will block for the given timeout. If timeout is None,
    /// the function will block indefinitely.
    ///
    /// This function returns `true` if the item was successfully enqueued, `false` otherwise.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    #[inline]
    pub unsafe fn send_to_front(&self, item: *const u8, timeout_us: Option<u32>) -> bool {
        unsafe { esp_rtos_queue_send_to_front(self.0, item, timeout_us) }
    }

    /// Enqueues a high-priority item.
    ///
    /// If the queue is full, this function will block until the deadline is reached. If the
    /// deadline is None, the function will block indefinitely.
    ///
    /// This function returns `true` if the item was successfully enqueued, `false` otherwise.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    #[inline]
    pub unsafe fn send_to_front_with_deadline(
        &self,
        item: *const u8,
        deadline_instant: Option<u64>,
    ) -> bool {
        unsafe { esp_rtos_queue_send_to_front_with_deadline(self.0, item, deadline_instant) }
    }

    /// Enqueues an item.
    ///
    /// If the queue is full, this function will block for the given timeout. If timeout is None,
    /// the function will block indefinitely.
    ///
    /// This function returns `true` if the item was successfully enqueued, `false` otherwise.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    #[inline]
    pub unsafe fn send_to_back(&self, item: *const u8, timeout_us: Option<u32>) -> bool {
        unsafe { esp_rtos_queue_send_to_back(self.0, item, timeout_us) }
    }

    /// Enqueues an item.
    ///
    /// If the queue is full, this function will block until the given deadline. If deadline is
    /// None, the function will block indefinitely.
    ///
    /// This function returns `true` if the item was successfully enqueued, `false` otherwise.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    #[inline]
    pub unsafe fn send_to_back_with_deadline(
        &self,
        item: *const u8,
        deadline_instant: Option<u64>,
    ) -> bool {
        unsafe { esp_rtos_queue_send_to_back_with_deadline(self.0, item, deadline_instant) }
    }

    /// Attempts to enqueues an item.
    ///
    /// If the queue is full, this function will immediately return `false`.
    ///
    /// If a higher priority task is woken up by this operation, the `higher_prio_task_waken` flag
    /// is set to `true`.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    #[inline]
    pub unsafe fn try_send_to_back_from_isr(
        &self,
        item: *const u8,
        higher_priority_task_waken: Option<&mut bool>,
    ) -> bool {
        unsafe {
            esp_rtos_queue_try_send_to_back_from_isr(self.0, item, higher_priority_task_waken)
        }
    }

    /// Dequeues an item from the queue.
    ///
    /// If the queue is empty, this function will block for the given timeout. If timeout is None,
    /// the function will block indefinitely.
    ///
    /// This function returns `true` if the item was successfully dequeued, `false` otherwise.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    #[inline]
    pub unsafe fn receive(&self, item: *mut u8, timeout_us: Option<u32>) -> bool {
        unsafe { esp_rtos_queue_receive(self.0, item, timeout_us) }
    }

    /// Dequeues an item from the queue.
    ///
    /// If the queue is empty, this function will block until the given deadline is reached. If
    /// deadline is None, the function will block indefinitely.
    ///
    /// This function returns `true` if the item was successfully dequeued, `false` otherwise.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    #[inline]
    pub unsafe fn receive_with_deadline(
        &self,
        item: *mut u8,
        deadline_instant: Option<u64>,
    ) -> bool {
        unsafe { esp_rtos_queue_receive_with_deadline(self.0, item, deadline_instant) }
    }

    /// Attempts to dequeue an item from the queue.
    ///
    /// If the queue is empty, this function will return `false` immediately.
    ///
    /// This function returns `true` if the item was successfully dequeued, `false` otherwise.
    ///
    /// If a higher priority task is woken up by this operation, the `higher_prio_task_waken` flag
    /// is set to `true`.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    #[inline]
    pub unsafe fn try_receive_from_isr(
        &self,
        item: *mut u8,
        higher_priority_task_waken: Option<&mut bool>,
    ) -> bool {
        unsafe { esp_rtos_queue_try_receive_from_isr(self.0, item, higher_priority_task_waken) }
    }

    /// Removes an item from the queue.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `item` can be dereferenced and points to an allocation of
    /// a size equal to the queue's item size.
    #[inline]
    pub unsafe fn remove(&self, item: *const u8) {
        unsafe { esp_rtos_queue_remove(self.0, item) }
    }

    /// Returns the number of messages in the queue.
    #[inline]
    pub fn messages_waiting(&self) -> usize {
        unsafe { esp_rtos_queue_messages_waiting(self.0) }
    }
}

impl Drop for QueueHandle {
    #[inline]
    fn drop(&mut self) {
        unsafe { esp_rtos_queue_delete(self.0) };
    }
}

#[cfg(feature = "ipc-implementations")]
mod implementation {
    use core::{
        cell::{RefCell, UnsafeCell},
        mem::MaybeUninit,
        ptr::NonNull,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use esp_sync::RawMutex;

    use super::*;
    use crate::{now, wait_queue::WaitQueueHandle};

    const CONTROL_CANARY_BEFORE: usize = 0x51a7_c0de;
    const CONTROL_CANARY_AFTER: usize = 0xcafe_71a5;
    const STORAGE_CANARY_BYTES: usize = core::mem::size_of::<usize>();
    const STORAGE_CANARY_BEFORE: u8 = 0xa5;
    const STORAGE_CANARY_AFTER: u8 = 0x5a;
    const COMPAT_QUEUE_SLOT_COUNT: usize = 8;
    const COMPAT_QUEUE_MAX_ITEM_BYTES: usize = 512;
    const COMPAT_QUEUE_MAX_PAYLOAD_BYTES: usize = 2 * 1024;
    const COMPAT_QUEUE_TOTAL_PAYLOAD_BYTES: usize = 2 * 1024;
    const COMPAT_QUEUE_WAIT_POLL_US: u64 = 1_000;
    const COMPAT_QUEUE_PAYLOAD_ARENA_BYTES: usize = COMPAT_QUEUE_TOTAL_PAYLOAD_BYTES
        + COMPAT_QUEUE_SLOT_COUNT * STORAGE_CANARY_BYTES * 2;
    const SLOT_EMPTY: usize = 0;
    const SLOT_INITIALIZING: usize = 1;
    const SLOT_ACTIVE: usize = 2;
    const SLOT_RETIRED: usize = 3;
    const SLOT_RECLAIMING: usize = 4;

    static CREATE_COUNT: AtomicUsize = AtomicUsize::new(0);
    static DELETE_COUNT: AtomicUsize = AtomicUsize::new(0);
    static RECLAIM_COUNT: AtomicUsize = AtomicUsize::new(0);
    static CORRUPTION_COUNT: AtomicUsize = AtomicUsize::new(0);
    static TASK_CONTENTION_REJECTED: AtomicUsize = AtomicUsize::new(0);
    static ISR_CONTENTION_REJECTED: AtomicUsize = AtomicUsize::new(0);
    static NONBLOCKING_CONTEXT_REDIRECTED: AtomicUsize = AtomicUsize::new(0);
    static SLOT_HIGH_WATER: AtomicUsize = AtomicUsize::new(0);
    static RESERVED_PAYLOAD_BYTES: AtomicUsize = AtomicUsize::new(0);

    struct PayloadArena(UnsafeCell<[u8; COMPAT_QUEUE_PAYLOAD_ARENA_BYTES]>);

    unsafe impl Sync for PayloadArena {}

    static PAYLOAD_ARENA: PayloadArena =
        PayloadArena(UnsafeCell::new([0; COMPAT_QUEUE_PAYLOAD_ARENA_BYTES]));
    static PAYLOAD_ARENA_LOCK: RawMutex = RawMutex::new();

    struct GuardedStorage {
        allocation_offset: usize,
        payload_len: usize,
    }

    impl GuardedStorage {
        fn new(allocation_offset: usize, payload_len: usize) -> Self {
            let mut storage = Self {
                allocation_offset,
                payload_len,
            };
            storage.allocation_mut().fill(0);
            storage.allocation_mut()[..STORAGE_CANARY_BYTES].fill(STORAGE_CANARY_BEFORE);
            storage.allocation_mut()[STORAGE_CANARY_BYTES + payload_len..]
                .fill(STORAGE_CANARY_AFTER);
            storage
        }

        fn allocation_len(&self) -> usize {
            self.payload_len + STORAGE_CANARY_BYTES * 2
        }

        fn allocation(&self) -> &[u8] {
            unsafe {
                core::slice::from_raw_parts(
                    (*PAYLOAD_ARENA.0.get())
                        .as_ptr()
                        .add(self.allocation_offset),
                    self.allocation_len(),
                )
            }
        }

        fn allocation_mut(&mut self) -> &mut [u8] {
            unsafe {
                core::slice::from_raw_parts_mut(
                    (*PAYLOAD_ARENA.0.get())
                        .as_mut_ptr()
                        .add(self.allocation_offset),
                    self.allocation_len(),
                )
            }
        }

        fn payload(&self) -> &[u8] {
            &self.allocation()[STORAGE_CANARY_BYTES..STORAGE_CANARY_BYTES + self.payload_len]
        }

        fn payload_mut(&mut self) -> &mut [u8] {
            let payload_len = self.payload_len;
            &mut self.allocation_mut()[STORAGE_CANARY_BYTES..STORAGE_CANARY_BYTES + payload_len]
        }

        fn canaries_intact(&self) -> bool {
            self.allocation()[..STORAGE_CANARY_BYTES]
                .iter()
                .all(|byte| *byte == STORAGE_CANARY_BEFORE)
                && self.allocation()[STORAGE_CANARY_BYTES + self.payload_len..]
                    .iter()
                .all(|byte| *byte == STORAGE_CANARY_AFTER)
        }

        fn first_canary_damage(&self) -> (usize, u8, usize, u8) {
            let allocation = self.allocation();
            let before = &allocation[..STORAGE_CANARY_BYTES];
            let after = &allocation[STORAGE_CANARY_BYTES + self.payload_len..];
            let before_index = before
                .iter()
                .position(|byte| *byte != STORAGE_CANARY_BEFORE)
                .unwrap_or(usize::MAX);
            let after_index = after
                .iter()
                .position(|byte| *byte != STORAGE_CANARY_AFTER)
                .unwrap_or(usize::MAX);
            (
                before_index,
                before.get(before_index).copied().unwrap_or(0),
                after_index,
                after.get(after_index).copied().unwrap_or(0),
            )
        }
    }

    struct QueueInner {
        storage: GuardedStorage,
        item_size: usize,
        capacity: usize,
        count: usize,
        current_read: usize,
        current_write: usize,
    }

    impl QueueInner {
        fn assert_canaries(&self, phase: &'static str) {
            if self.storage.canaries_intact() {
                return;
            }
            CORRUPTION_COUNT.fetch_add(1, Ordering::Relaxed);
            let (before_index, before_value, after_index, after_value) =
                self.storage.first_canary_damage();
            panic!(
                "compat queue storage canary corrupted phase={} before_index={} before_value=0x{:02x} after_index={} after_value=0x{:02x} payload_len={} item_size={} capacity={} count={} read={} write={}",
                phase,
                before_index,
                before_value,
                after_index,
                after_value,
                self.storage.payload_len,
                self.item_size,
                self.capacity,
                self.count,
                self.current_read,
                self.current_write,
            );
        }

        fn get(&self, index: usize) -> &[u8] {
            let item_start = self.item_size * index;
            &self.storage.payload()[item_start..][..self.item_size]
        }

        fn get_mut(&mut self, index: usize) -> &mut [u8] {
            let item_start = self.item_size * index;
            &mut self.storage.payload_mut()[item_start..][..self.item_size]
        }

        fn len(&self) -> usize {
            self.count
        }

        fn send_to_back(&mut self, item: *const u8) {
            let item = unsafe { core::slice::from_raw_parts(item, self.item_size) };

            let dst = self.get_mut(self.current_write);
            dst.copy_from_slice(item);

            self.current_write = (self.current_write + 1) % self.capacity;
            self.count += 1;
        }

        fn send_to_front(&mut self, item: *const u8) {
            let item = unsafe { core::slice::from_raw_parts(item, self.item_size) };

            self.current_read = (self.current_read + self.capacity - 1) % self.capacity;

            let dst = self.get_mut(self.current_read);
            dst.copy_from_slice(item);

            self.count += 1;
        }

        fn read_from_front(&mut self, dst: *mut u8) {
            let dst = unsafe { core::slice::from_raw_parts_mut(dst, self.item_size) };

            let src = self.get(self.current_read);
            dst.copy_from_slice(src);

            self.current_read = (self.current_read + 1) % self.capacity;
            self.count -= 1;
        }

        fn remove(&mut self, item: *const u8) -> bool {
            let item_size = self.item_size;
            let item_slice = unsafe { core::slice::from_raw_parts(item, self.item_size) };
            let Some(found) = (0..self.count).find(|offset| {
                let index = (self.current_read + offset) % self.capacity;
                self.get(index) == item_slice
            }) else {
                return false;
            };

            for offset in found..self.count - 1 {
                let destination = (self.current_read + offset) % self.capacity;
                let source = (self.current_read + offset + 1) % self.capacity;
                let source_start = source * item_size;
                let destination_start = destination * item_size;
                self.storage.payload_mut().copy_within(
                    source_start..source_start + item_size,
                    destination_start,
                );
            }
            self.current_write = (self.current_write + self.capacity - 1) % self.capacity;
            self.count -= 1;
            true
        }
    }

    /// A bounded queue implementation with a per-queue raw lock.
    ///
    /// Register in your OS implementation by adding the following code:
    ///
    /// ```rust
    /// use esp_radio_rtos_driver::{queue::CompatQueue, register_queue_implementation};
    ///
    /// register_queue_implementation!(CompatQueue);
    /// ```
    pub struct CompatQueue {
        /// The raw lock is reentrant; RefCell rejects a nested same-core operation.
        lock: RawMutex,
        inner: RefCell<QueueInner>,
        waiting_for_space: WaitQueueHandle,
        waiting_for_items: WaitQueueHandle,
    }

    unsafe impl Sync for CompatQueue {}

    #[repr(C)]
    struct CompatQueueSlot {
        state: AtomicUsize,
        payload_bytes: AtomicUsize,
        allocation_offset: AtomicUsize,
        allocation_bytes: AtomicUsize,
        canary_before: AtomicUsize,
        value: UnsafeCell<MaybeUninit<CompatQueue>>,
        canary_after: AtomicUsize,
    }

    // Slot state serializes initialization and reclamation. Queue operations are
    // additionally serialized by CompatQueue's own raw lock.
    unsafe impl Sync for CompatQueueSlot {}

    impl CompatQueueSlot {
        const fn new() -> Self {
            Self {
                state: AtomicUsize::new(SLOT_EMPTY),
                payload_bytes: AtomicUsize::new(0),
                allocation_offset: AtomicUsize::new(usize::MAX),
                allocation_bytes: AtomicUsize::new(0),
                canary_before: AtomicUsize::new(CONTROL_CANARY_BEFORE),
                value: UnsafeCell::new(MaybeUninit::uninit()),
                canary_after: AtomicUsize::new(CONTROL_CANARY_AFTER),
            }
        }

        fn value_ptr(&self) -> *mut CompatQueue {
            unsafe { (*self.value.get()).as_mut_ptr() }
        }

        fn canaries_intact(&self) -> bool {
            self.canary_before.load(Ordering::Acquire) == CONTROL_CANARY_BEFORE
                && self.canary_after.load(Ordering::Acquire) == CONTROL_CANARY_AFTER
        }
    }

    static COMPAT_QUEUE_SLOTS: [CompatQueueSlot; COMPAT_QUEUE_SLOT_COUNT] =
        [const { CompatQueueSlot::new() }; COMPAT_QUEUE_SLOT_COUNT];

    /// Snapshot of the fixed-capacity compat-queue owner.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct CompatQueuePoolStats {
        pub created: usize,
        pub deleted: usize,
        pub active: usize,
        pub retired: usize,
        pub reclaimed: usize,
        pub corruption: usize,
        pub task_contention_rejected: usize,
        pub isr_contention_rejected: usize,
        pub nonblocking_context_redirected: usize,
        pub reserved_payload_bytes: usize,
        pub payload_capacity_bytes: usize,
        pub slot_high_water: usize,
        pub slot_capacity: usize,
    }

    fn update_high_water(value: usize) {
        let mut high = SLOT_HIGH_WATER.load(Ordering::Relaxed);
        while value > high {
            match SLOT_HIGH_WATER.compare_exchange_weak(
                high,
                value,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => high = observed,
            }
        }
    }

    fn reserve_payload(slot_index: usize, payload_bytes: usize) -> Option<GuardedStorage> {
        if payload_bytes > COMPAT_QUEUE_MAX_PAYLOAD_BYTES {
            return None;
        }
        let allocation_bytes = payload_bytes.checked_add(STORAGE_CANARY_BYTES * 2)?;
        PAYLOAD_ARENA_LOCK.lock(|| {
            let mut candidate = 0usize;
            let allocation_offset = loop {
                let mut next_offset = usize::MAX;
                let mut next_bytes = 0usize;
                for (index, slot) in COMPAT_QUEUE_SLOTS.iter().enumerate() {
                    if index == slot_index {
                        continue;
                    }
                    let occupied_bytes = slot.allocation_bytes.load(Ordering::Acquire);
                    if occupied_bytes == 0 {
                        continue;
                    }
                    let occupied_offset = slot.allocation_offset.load(Ordering::Acquire);
                    if occupied_offset >= candidate && occupied_offset < next_offset {
                        next_offset = occupied_offset;
                        next_bytes = occupied_bytes;
                    }
                }

                let candidate_end = candidate.checked_add(allocation_bytes)?;
                if next_offset == usize::MAX {
                    if candidate_end <= COMPAT_QUEUE_PAYLOAD_ARENA_BYTES {
                        break candidate;
                    }
                    return None;
                }
                if candidate_end <= next_offset {
                    break candidate;
                }
                candidate = next_offset.checked_add(next_bytes)?;
            };

            let slot = &COMPAT_QUEUE_SLOTS[slot_index];
            slot.payload_bytes.store(payload_bytes, Ordering::Relaxed);
            slot.allocation_offset
                .store(allocation_offset, Ordering::Relaxed);
            slot.allocation_bytes
                .store(allocation_bytes, Ordering::Release);
            RESERVED_PAYLOAD_BYTES.fetch_add(payload_bytes, Ordering::AcqRel);
            Some(GuardedStorage::new(allocation_offset, payload_bytes))
        })
    }

    fn release_payload(slot: &CompatQueueSlot) {
        PAYLOAD_ARENA_LOCK.lock(|| {
            let payload_bytes = slot.payload_bytes.swap(0, Ordering::Relaxed);
            let allocation_offset = slot.allocation_offset.swap(usize::MAX, Ordering::Relaxed);
            let allocation_bytes = slot.allocation_bytes.swap(0, Ordering::AcqRel);
            if allocation_bytes != 0 {
                unsafe {
                    core::slice::from_raw_parts_mut(
                        (*PAYLOAD_ARENA.0.get()).as_mut_ptr().add(allocation_offset),
                        allocation_bytes,
                    )
                    .fill(0);
                }
                RESERVED_PAYLOAD_BYTES.fetch_sub(payload_bytes, Ordering::AcqRel);
            }
        });
    }

    fn slot_for(queue: QueuePtr) -> Option<&'static CompatQueueSlot> {
        let pointer = queue.as_ptr().cast::<CompatQueue>();
        COMPAT_QUEUE_SLOTS
            .iter()
            .find(|slot| slot.value_ptr() == pointer)
    }

    fn queue_for_active_slot(queue: QueuePtr) -> Option<&'static CompatQueue> {
        let slot = slot_for(queue)?;
        if slot.state.load(Ordering::Acquire) != SLOT_ACTIVE {
            return None;
        }
        if !slot.canaries_intact() {
            CORRUPTION_COUNT.fetch_add(1, Ordering::Relaxed);
            panic!("compat queue control canary corrupted");
        }
        Some(unsafe { &*slot.value_ptr() })
    }

    /// Reclaim a retired queue after its external callback source is quiescent.
    ///
    /// The caller must prove that no future operation can arrive for `queue`.
    pub unsafe fn compat_queue_reclaim(queue: QueuePtr) -> bool {
        let Some(slot) = slot_for(queue) else {
            return false;
        };
        if slot
            .state
            .compare_exchange(
                SLOT_RETIRED,
                SLOT_RECLAIMING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return false;
        }
        if !slot.canaries_intact() {
            CORRUPTION_COUNT.fetch_add(1, Ordering::Relaxed);
            panic!("compat queue control canary corrupted");
        }
        let queue = unsafe { &*slot.value_ptr() };
        if !queue.canaries_intact() {
            CORRUPTION_COUNT.fetch_add(1, Ordering::Relaxed);
            panic!("compat queue storage canary corrupted");
        }
        unsafe { slot.value_ptr().drop_in_place() };
        release_payload(slot);
        slot.state.store(SLOT_EMPTY, Ordering::Release);
        RECLAIM_COUNT.fetch_add(1, Ordering::Relaxed);
        true
    }

    pub fn compat_queue_pool_stats() -> CompatQueuePoolStats {
        let mut stats = CompatQueuePoolStats {
            created: CREATE_COUNT.load(Ordering::Relaxed),
            deleted: DELETE_COUNT.load(Ordering::Relaxed),
            reclaimed: RECLAIM_COUNT.load(Ordering::Relaxed),
            corruption: CORRUPTION_COUNT.load(Ordering::Relaxed),
            task_contention_rejected: TASK_CONTENTION_REJECTED.load(Ordering::Relaxed),
            isr_contention_rejected: ISR_CONTENTION_REJECTED.load(Ordering::Relaxed),
            nonblocking_context_redirected: NONBLOCKING_CONTEXT_REDIRECTED.load(Ordering::Relaxed),
            reserved_payload_bytes: RESERVED_PAYLOAD_BYTES.load(Ordering::Acquire),
            payload_capacity_bytes: COMPAT_QUEUE_TOTAL_PAYLOAD_BYTES,
            slot_high_water: SLOT_HIGH_WATER.load(Ordering::Relaxed),
            slot_capacity: COMPAT_QUEUE_SLOT_COUNT,
            ..CompatQueuePoolStats::default()
        };
        for slot in &COMPAT_QUEUE_SLOTS {
            match slot.state.load(Ordering::Acquire) {
                SLOT_ACTIVE => stats.active += 1,
                SLOT_RETIRED => stats.retired += 1,
                _ => {}
            }
        }
        stats
    }

    impl CompatQueue {
        #[inline]
        fn in_nonblocking_context() -> bool {
            #[cfg(target_arch = "xtensa")]
            {
                xtensa_lx::interrupt::get_level() != 0
            }
            #[cfg(not(target_arch = "xtensa"))]
            {
                false
            }
        }

        fn new(capacity: usize, item_size: usize, storage: GuardedStorage) -> Self {
            assert!(capacity > 0, "queue capacity must be non-zero");
            assert!(item_size > 0, "queue item size must be non-zero");
            assert!(
                item_size <= COMPAT_QUEUE_MAX_ITEM_BYTES,
                "queue item exceeds critical-section copy ceiling"
            );
            Self {
                lock: RawMutex::new(),
                inner: RefCell::new(QueueInner {
                    storage,
                    item_size,
                    capacity,
                    count: 0,
                    current_read: 0,
                    current_write: 0,
                }),
                waiting_for_space: WaitQueueHandle::new(),
                waiting_for_items: WaitQueueHandle::new(),
            }
        }

        fn try_with<R>(&self, f: impl FnOnce(&mut QueueInner) -> R) -> Option<R> {
            self.lock.lock(|| {
                let mut inner = self.inner.try_borrow_mut().ok()?;
                inner.assert_canaries("before_operation");
                let result = f(&mut inner);
                inner.assert_canaries("after_operation");
                Some(result)
            })
        }

        fn canaries_intact(&self) -> bool {
            self.lock.lock(|| {
                self.inner
                    .try_borrow()
                    .is_ok_and(|inner| inner.storage.canaries_intact())
            })
        }

        fn wait_until(
            &self,
            deadline: Option<u64>,
            waiting: &WaitQueueHandle,
            mut operation: impl FnMut(&mut QueueInner) -> bool,
        ) -> bool {
            if Self::in_nonblocking_context() {
                NONBLOCKING_CONTEXT_REDIRECTED.fetch_add(1, Ordering::Relaxed);
                return self.try_with(|inner| operation(inner)).unwrap_or_else(|| {
                    ISR_CONTENTION_REJECTED.fetch_add(1, Ordering::Relaxed);
                    false
                });
            }
            loop {
                let result = self.lock.lock(|| {
                    let mut inner = self.inner.try_borrow_mut().ok()?;
                    inner.assert_canaries("before_wait_operation");
                    let completed = operation(&mut inner);
                    inner.assert_canaries("after_wait_operation");
                    if completed {
                        Some(true)
                    } else {
                        let poll_deadline = now().saturating_add(COMPAT_QUEUE_WAIT_POLL_US);
                        waiting.wait_until(Some(
                            deadline.map_or(poll_deadline, |deadline| deadline.min(poll_deadline)),
                        ));
                        Some(false)
                    }
                });
                if result == Some(true) {
                    return true;
                }
                if result.is_none() {
                    TASK_CONTENTION_REJECTED.fetch_add(1, Ordering::Relaxed);
                    return false;
                }
                if deadline.is_some_and(|deadline| now() >= deadline) {
                    return false;
                }
            }
        }
    }

    impl QueueImplementation for CompatQueue {
        fn create(capacity: usize, item_size: usize) -> QueuePtr {
            let payload_bytes = capacity
                .checked_mul(item_size)
                .expect("queue payload length overflow");
            for (index, slot) in COMPAT_QUEUE_SLOTS.iter().enumerate() {
                if slot
                    .state
                    .compare_exchange(
                        SLOT_EMPTY,
                        SLOT_INITIALIZING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    assert!(slot.canaries_intact(), "compat queue control canary corrupted");
                    let Some(storage) = reserve_payload(index, payload_bytes) else {
                        slot.state.store(SLOT_EMPTY, Ordering::Release);
                        panic!("fixed compat queue payload arena exhausted");
                    };
                    unsafe {
                        slot.value_ptr()
                            .write(CompatQueue::new(capacity, item_size, storage))
                    };
                    slot.state.store(SLOT_ACTIVE, Ordering::Release);
                    CREATE_COUNT.fetch_add(1, Ordering::Relaxed);
                    update_high_water(index + 1);
                    return NonNull::new(slot.value_ptr().cast())
                        .expect("static compat queue slot is null");
                }
            }
            panic!("fixed compat queue pool exhausted");
        }

        unsafe fn delete(queue: QueuePtr) {
            let slot = slot_for(queue).expect("compat queue delete used unknown pointer");
            assert!(slot.canaries_intact(), "compat queue control canary corrupted");
            match slot.state.compare_exchange(
                SLOT_ACTIVE,
                SLOT_RETIRED,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    DELETE_COUNT.fetch_add(1, Ordering::Relaxed);
                }
                Err(SLOT_RETIRED) => panic!("compat queue deleted twice"),
                Err(_) => panic!("compat queue delete used inactive slot"),
            }
        }

        unsafe fn send_to_front(queue: QueuePtr, item: *const u8, timeout_us: Option<u32>) -> bool {
            let deadline_instant = timeout_us.map(|timeout| now() + timeout as u64);
            unsafe { Self::send_to_front_with_deadline(queue, item, deadline_instant) }
        }

        unsafe fn send_to_front_with_deadline(
            queue: QueuePtr,
            item: *const u8,
            deadline_instant: Option<u64>,
        ) -> bool {
            let Some(queue) = queue_for_active_slot(queue) else {
                return false;
            };

            let may_notify = !CompatQueue::in_nonblocking_context();
            let sent = queue.wait_until(deadline_instant, &queue.waiting_for_space, |inner| {
                if inner.len() == inner.capacity {
                    false
                } else {
                    inner.send_to_front(item);
                    true
                }
            });
            if sent && may_notify {
                queue.waiting_for_items.notify();
            }
            sent
        }

        unsafe fn send_to_back(queue: QueuePtr, item: *const u8, timeout_us: Option<u32>) -> bool {
            let deadline_instant = timeout_us.map(|timeout| now() + timeout as u64);
            unsafe { Self::send_to_back_with_deadline(queue, item, deadline_instant) }
        }

        unsafe fn send_to_back_with_deadline(
            queue: QueuePtr,
            item: *const u8,
            deadline_instant: Option<u64>,
        ) -> bool {
            let Some(queue) = queue_for_active_slot(queue) else {
                return false;
            };

            let may_notify = !CompatQueue::in_nonblocking_context();
            let sent = queue.wait_until(deadline_instant, &queue.waiting_for_space, |inner| {
                if inner.len() == inner.capacity {
                    false
                } else {
                    inner.send_to_back(item);
                    true
                }
            });
            if sent && may_notify {
                queue.waiting_for_items.notify();
            }
            sent
        }

        unsafe fn try_send_to_back_from_isr(
            queue: QueuePtr,
            item: *const u8,
            higher_prio_task_waken: Option<&mut bool>,
        ) -> bool {
            let Some(queue) = queue_for_active_slot(queue) else {
                return false;
            };

            let sent = match queue.try_with(|inner| {
                if inner.len() == inner.capacity {
                    false
                } else {
                    inner.send_to_back(item);
                    true
                }
            }) {
                Some(result) => result,
                None => {
                    ISR_CONTENTION_REJECTED.fetch_add(1, Ordering::Relaxed);
                    false
                }
            };
            if let Some(higher_prio_task_waken) = higher_prio_task_waken {
                *higher_prio_task_waken = false;
            }
            // esp-rtos' ISR notify currently enters its task scheduler. The bounded
            // task wait above observes this send at the next 1 ms timer deadline.
            sent
        }

        unsafe fn receive(queue: QueuePtr, item: *mut u8, timeout_us: Option<u32>) -> bool {
            let deadline_instant = timeout_us.map(|timeout| now() + timeout as u64);
            unsafe { Self::receive_with_deadline(queue, item, deadline_instant) }
        }

        unsafe fn receive_with_deadline(
            queue: QueuePtr,
            item: *mut u8,
            deadline_instant: Option<u64>,
        ) -> bool {
            let Some(queue) = queue_for_active_slot(queue) else {
                return false;
            };

            let may_notify = !CompatQueue::in_nonblocking_context();
            let received = queue.wait_until(deadline_instant, &queue.waiting_for_items, |inner| {
                if inner.len() == 0 {
                    false
                } else {
                    inner.read_from_front(item);
                    true
                }
            });
            if received && may_notify {
                queue.waiting_for_space.notify();
            }
            received
        }

        unsafe fn try_receive_from_isr(
            queue: QueuePtr,
            item: *mut u8,
            higher_prio_task_waken: Option<&mut bool>,
        ) -> bool {
            let Some(queue) = queue_for_active_slot(queue) else {
                return false;
            };

            let received = match queue.try_with(|inner| {
                if inner.len() == 0 {
                    false
                } else {
                    inner.read_from_front(item);
                    true
                }
            }) {
                Some(result) => result,
                None => {
                    ISR_CONTENTION_REJECTED.fetch_add(1, Ordering::Relaxed);
                    false
                }
            };
            if let Some(higher_prio_task_waken) = higher_prio_task_waken {
                *higher_prio_task_waken = false;
            }
            // Task waiters poll at the bounded timer deadline; do not enter the
            // esp-rtos scheduler from this ISR callback.
            received
        }

        unsafe fn remove(queue: QueuePtr, item: *const u8) {
            let Some(queue) = queue_for_active_slot(queue) else {
                return;
            };

            let may_notify = !CompatQueue::in_nonblocking_context();
            if queue.try_with(|inner| inner.remove(item)) == Some(true) && may_notify {
                queue.waiting_for_space.notify();
            }
        }

        fn messages_waiting(queue: QueuePtr) -> usize {
            let Some(queue) = queue_for_active_slot(queue) else {
                return 0;
            };

            queue.try_with(|inner| inner.len()).unwrap_or(0)
        }
    }
}

#[cfg(feature = "ipc-implementations")]
pub use implementation::{
    CompatQueue, CompatQueuePoolStats, compat_queue_pool_stats, compat_queue_reclaim,
};
