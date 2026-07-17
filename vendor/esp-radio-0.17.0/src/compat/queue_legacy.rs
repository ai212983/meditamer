use allocator_api2::{boxed::Box, vec::Vec};
use core::{cell::UnsafeCell, mem::size_of_val, ptr};

use esp_wifi_sys::c_types::{c_int, c_void};

use crate::{
    compat::{
        OSI_FUNCS_TIME_BLOCKING,
        malloc::{InternalMemory, free, malloc},
    },
    memory_fence::memory_fence,
    time,
};

struct Locked<T> {
    inner: UnsafeCell<T>,
}

unsafe impl<T> Sync for Locked<T> {}

impl<T> Locked<T> {
    const fn new(value: T) -> Self {
        Self {
            inner: UnsafeCell::new(value),
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
    fn new(count: usize, item_size: usize) -> Self {
        Self {
            raw_queue: Locked::new(RawQueue::new(count, item_size)),
        }
    }

    fn enqueue(&mut self, item: *const c_void) -> i32 {
        self.raw_queue.with(|q| unsafe { q.enqueue(item) })
    }

    fn enqueue_front(&mut self, item: *const c_void) -> i32 {
        self.raw_queue.with(|q| unsafe { q.enqueue_front(item) })
    }

    fn try_dequeue(&mut self, item: *mut c_void) -> bool {
        self.raw_queue.with(|q| unsafe { q.try_dequeue(item) })
    }

    fn remove(&mut self, item: *const c_void) {
        self.raw_queue.with(|q| unsafe { q.remove(item) })
    }

    fn count(&self) -> usize {
        self.raw_queue.with(|q| q.count())
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
        let start = self.item_size * index;
        &self.storage[start..][..self.item_size]
    }

    fn get_mut(&mut self, index: usize) -> &mut [u8] {
        let start = self.item_size * index;
        &mut self.storage[start..][..self.item_size]
    }

    fn count(&self) -> usize {
        if self.current_write >= self.current_read {
            self.current_write - self.current_read
        } else {
            self.capacity - self.current_read + self.current_write
        }
    }

    fn full(&self) -> bool {
        self.count() == self.capacity
    }

    fn empty(&self) -> bool {
        self.count() == 0
    }

    unsafe fn enqueue(&mut self, item: *const c_void) -> i32 {
        if self.full() {
            return 0;
        }

        let src = unsafe { core::slice::from_raw_parts(item.cast::<u8>(), self.item_size) };
        self.get_mut(self.current_write).copy_from_slice(src);
        self.current_write = (self.current_write + 1) % self.capacity;
        1
    }

    unsafe fn enqueue_front(&mut self, item: *const c_void) -> i32 {
        if self.full() {
            return 0;
        }

        let src = unsafe { core::slice::from_raw_parts(item.cast::<u8>(), self.item_size) };
        self.current_read = (self.current_read + self.capacity - 1) % self.capacity;
        self.get_mut(self.current_read).copy_from_slice(src);
        1
    }

    unsafe fn try_dequeue(&mut self, item: *mut c_void) -> bool {
        if self.empty() {
            return false;
        }

        let dst = unsafe { core::slice::from_raw_parts_mut(item.cast::<u8>(), self.item_size) };
        dst.copy_from_slice(self.get(self.current_read));
        self.current_read = (self.current_read + 1) % self.capacity;
        true
    }

    unsafe fn remove(&mut self, item: *const c_void) {
        let item_slice = unsafe { core::slice::from_raw_parts(item.cast::<u8>(), self.item_size) };
        let count = self.count();
        if count == 0 {
            return;
        }

        let mut tmp = Vec::<u8, _>::new_in(InternalMemory);
        tmp.reserve_exact(self.item_size);
        tmp.resize(self.item_size, 0);

        for _ in 0..count {
            if !unsafe { self.try_dequeue(tmp.as_mut_ptr().cast()) } {
                break;
            }
            if &tmp[..] != item_slice {
                unsafe { self.enqueue(tmp.as_ptr().cast()) };
            }
        }
    }
}

fn queue_ptr(queue: *mut c_void) -> *mut ConcurrentQueue {
    queue.cast()
}

pub(crate) fn queue_create(queue_len: c_int, item_size: c_int) -> *mut c_void {
    trace!("legacy queue_create len={} size={}", queue_len, item_size);
    let queue = ConcurrentQueue::new(queue_len as usize, item_size as usize);
    let ptr = unsafe { malloc(size_of_val(&queue)) as *mut ConcurrentQueue };
    unsafe {
        ptr.write(queue);
    }
    ptr.cast()
}

pub(crate) fn queue_delete(queue: *mut c_void) {
    let ptr = queue_ptr(queue);
    unsafe {
        ptr::drop_in_place(ptr);
        free(ptr.cast());
    }
}

pub(crate) fn queue_send_to_back(queue: *mut c_void, item: *const c_void, tick: u32) -> i32 {
    let forever = tick == OSI_FUNCS_TIME_BLOCKING;
    let timeout = tick as u64;
    let start = time::systimer_count();
    let queue = queue_ptr(queue);

    loop {
        let sent = unsafe { (*queue).enqueue(item) };
        if sent == 1 {
            return 1;
        }
        if !forever && time::elapsed_time_since(start) > timeout {
            return 0;
        }
        crate::preempt::yield_task();
    }
}

pub(crate) fn queue_try_send_to_back_from_isr(
    queue: *mut c_void,
    item: *const c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    let sent = unsafe { (*queue_ptr(queue)).enqueue(item) };
    if sent == 1 {
        unsafe {
            if let Some(waken) = higher_priority_task_waken.as_mut() {
                *waken = true;
            }
        }
    }
    sent
}

pub(crate) fn queue_send_to_front(queue: *mut c_void, item: *const c_void, tick: u32) -> i32 {
    let forever = tick == OSI_FUNCS_TIME_BLOCKING;
    let timeout = tick as u64;
    let start = time::systimer_count();
    let queue = queue_ptr(queue);

    loop {
        let sent = unsafe { (*queue).enqueue_front(item) };
        if sent == 1 {
            return 1;
        }
        if !forever && time::elapsed_time_since(start) > timeout {
            return 0;
        }
        crate::preempt::yield_task();
    }
}

pub(crate) fn queue_receive(queue: *mut c_void, item: *mut c_void, tick: u32) -> i32 {
    let forever = tick == OSI_FUNCS_TIME_BLOCKING;
    let timeout = tick as u64;
    let start = time::systimer_count();
    let queue = queue_ptr(queue);

    loop {
        if unsafe { (*queue).try_dequeue(item) } {
            return 1;
        }
        if !forever && time::elapsed_time_since(start) > timeout {
            return -1;
        }
        crate::preempt::yield_task();
    }
}

pub(crate) fn queue_try_receive_from_isr(
    queue: *mut c_void,
    item: *mut c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    let received = unsafe { (*queue_ptr(queue)).try_dequeue(item) };
    if received {
        unsafe {
            if let Some(waken) = higher_priority_task_waken.as_mut() {
                *waken = true;
            }
        }
        1
    } else {
        0
    }
}

pub(crate) fn queue_remove(queue: *mut c_void, item: *const c_void) {
    unsafe { (*queue_ptr(queue)).remove(item) };
    memory_fence();
}

pub(crate) fn queue_messages_waiting(queue: *mut c_void) -> u32 {
    unsafe { (*queue_ptr(queue)).count() as u32 }
}
