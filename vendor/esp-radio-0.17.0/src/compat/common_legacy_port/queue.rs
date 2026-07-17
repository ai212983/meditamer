use core::{ffi::c_void, mem::size_of, ptr};

use allocator_api2::{boxed::Box, vec::Vec};

use crate::{
    compat::{malloc::{free, malloc, InternalMemory}, OSI_FUNCS_TIME_BLOCKING},
};

struct Locked<T> {
    inner: core::cell::UnsafeCell<T>,
}

unsafe impl<T> Sync for Locked<T> {}

impl<T> Locked<T> {
    const fn new(value: T) -> Self {
        Self {
            inner: core::cell::UnsafeCell::new(value),
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        critical_section::with(|_| f(unsafe { &mut *self.inner.get() }))
    }
}

pub(crate) struct ConcurrentQueue {
    raw_queue: Locked<RawQueue>,
}

impl ConcurrentQueue {
    pub(crate) fn new(count: usize, item_size: usize) -> Self {
        Self {
            raw_queue: Locked::new(RawQueue::new(count, item_size)),
        }
    }

    pub(crate) fn enqueue(&self, item: *mut c_void) -> i32 {
        self.raw_queue.with(|q| unsafe { q.enqueue(item) })
    }

    pub(crate) fn try_dequeue(&self, item: *mut c_void) -> bool {
        self.raw_queue.with(|q| unsafe { q.try_dequeue(item) })
    }

    pub(crate) fn count(&self) -> usize {
        self.raw_queue.with(|q| q.count())
    }

    pub(crate) fn remove(&self, item: *mut c_void) {
        self.raw_queue.with(|q| unsafe { q.remove(item) });
    }
}

struct RawQueue {
    item_size: usize,
    capacity: usize,
    current_read: usize,
    current_write: usize,
    storage: Box<[u8], InternalMemory>,
}

impl RawQueue {
    fn new(capacity: usize, item_size: usize) -> Self {
        let storage =
            unsafe { Box::new_zeroed_slice_in(capacity * item_size, InternalMemory).assume_init() };

        Self {
            item_size,
            capacity,
            current_read: 0,
            current_write: 0,
            storage,
        }
    }

    fn get(&self, index: usize) -> &[u8] {
        let item_start = self.item_size * index;
        &self.storage[item_start..][..self.item_size]
    }

    fn get_mut(&mut self, index: usize) -> &mut [u8] {
        let item_start = self.item_size * index;
        &mut self.storage[item_start..][..self.item_size]
    }

    fn full(&self) -> bool {
        self.count() == self.capacity
    }

    fn empty(&self) -> bool {
        self.count() == 0
    }

    unsafe fn enqueue(&mut self, item: *mut c_void) -> i32 {
        if self.full() {
            return 0;
        }

        let item = unsafe { core::slice::from_raw_parts(item as *const u8, self.item_size) };
        let dst = self.get_mut(self.current_write);
        dst.copy_from_slice(item);
        self.current_write = (self.current_write + 1) % self.capacity;
        1
    }

    unsafe fn try_dequeue(&mut self, item: *mut c_void) -> bool {
        if self.empty() {
            return false;
        }

        let item = unsafe { core::slice::from_raw_parts_mut(item as *mut u8, self.item_size) };
        let src = self.get(self.current_read);
        item.copy_from_slice(src);
        self.current_read = (self.current_read + 1) % self.capacity;
        true
    }

    unsafe fn remove(&mut self, item: *mut c_void) {
        let item_slice = unsafe { core::slice::from_raw_parts(item as *const u8, self.item_size) };
        let count = self.count();
        if count == 0 {
            return;
        }

        let mut tmp_item = Vec::<u8, _>::new_in(InternalMemory);
        tmp_item.reserve_exact(self.item_size);
        tmp_item.resize(self.item_size, 0);

        for _ in 0..count {
            if !unsafe { self.try_dequeue(tmp_item.as_mut_ptr().cast()) } {
                break;
            }
            if &tmp_item[..] != item_slice {
                unsafe { self.enqueue(tmp_item.as_mut_ptr().cast()) };
            }
        }
    }

    fn count(&self) -> usize {
        if self.current_write >= self.current_read {
            self.current_write - self.current_read
        } else {
            self.capacity - self.current_read + self.current_write
        }
    }
}

pub(crate) unsafe fn queue_create(queue_len: u32, item_size: u32) -> *mut c_void {
    let queue = ConcurrentQueue::new(queue_len as usize, item_size as usize);
    let ptr = unsafe { malloc(size_of::<ConcurrentQueue>()) as *mut ConcurrentQueue };
    unsafe { ptr.write(queue) };
    ptr.cast()
}

pub(crate) unsafe fn queue_delete(queue: *mut c_void) {
    let queue = queue.cast::<ConcurrentQueue>();
    unsafe {
        ptr::drop_in_place(queue);
        free(queue.cast());
    }
}

pub(crate) unsafe fn queue_send_to_back(
    queue: *mut c_void,
    item: *mut c_void,
    _block_time_tick: u32,
) -> i32 {
    unsafe { (*queue.cast::<ConcurrentQueue>()).enqueue(item) }
}

pub(crate) unsafe fn queue_send_to_front(
    queue: *mut c_void,
    item: *mut c_void,
    _block_time_tick: u32,
) -> i32 {
    // Legacy common.rs uses a single ring queue and front insert is rare in our path.
    // Keep behavior simple by using the same enqueue path as the legacy queue shim.
    unsafe { (*queue.cast::<ConcurrentQueue>()).enqueue(item) }
}

pub(crate) unsafe fn queue_send_from_isr(
    queue: *mut c_void,
    item: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    if !higher_priority_task_waken.is_null() {
        unsafe { *(higher_priority_task_waken as *mut u32) = 1 };
    }
    unsafe { (*queue.cast::<ConcurrentQueue>()).enqueue(item) }
}

pub(crate) unsafe fn queue_recv(
    queue: *mut c_void,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    let forever = block_time_tick == OSI_FUNCS_TIME_BLOCKING;
    let timeout = block_time_tick as u64;
    let start = crate::time::systimer_count();
    let queue = queue.cast::<ConcurrentQueue>();

    loop {
        if unsafe { (*queue).try_dequeue(item) } {
            return 1;
        }
        if !forever && crate::time::elapsed_time_since(start) > timeout {
            return -1;
        }
        crate::compat::preempt_legacy_backend::yield_task();
    }
}

pub(crate) unsafe fn queue_recv_from_isr(
    queue: *mut c_void,
    item: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    let ok = unsafe { (*queue.cast::<ConcurrentQueue>()).try_dequeue(item) };
    if ok {
        if !higher_priority_task_waken.is_null() {
            unsafe { *(higher_priority_task_waken as *mut u32) = 1 };
        }
        1
    } else {
        0
    }
}

pub(crate) unsafe fn queue_msg_waiting(queue: *mut c_void) -> u32 {
    unsafe { (*queue.cast::<ConcurrentQueue>()).count() as u32 }
}
