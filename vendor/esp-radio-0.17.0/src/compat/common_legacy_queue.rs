use allocator_api2::{boxed::Box, vec::Vec};
use core::{cell::UnsafeCell, mem::size_of_val, ptr};
use portable_atomic::{AtomicU32, AtomicUsize, Ordering};

use esp_wifi_sys::c_types::{c_int, c_void};

use crate::{
    compat::{
        OSI_FUNCS_TIME_BLOCKING,
        malloc::{InternalMemory, free, malloc},
        preempt_legacy_backend as legacy_preempt,
    },
    memory_fence::memory_fence,
    time,
};

#[derive(Clone, Copy)]
pub(crate) struct CommonLegacyQueueDiag {
    pub send_count: u32,
    pub send_front_count: u32,
    pub recv_count: u32,
    pub send_isr_count: u32,
    pub recv_isr_count: u32,
    pub last_queue_ptr: usize,
    pub recent_send_ordinals: [u32; 8],
    pub recent_send_item_word0: [u32; 8],
    pub recent_send_item_pointee_word0: [u32; 8],
    pub recent_send_item_pointee_word1: [u32; 8],
    pub recent_recv_ordinals: [u32; 8],
    pub recent_recv_item_word0: [u32; 8],
    pub recent_recv_item_pointee_word0: [u32; 8],
    pub recent_recv_item_pointee_word1: [u32; 8],
}

static COMMON_LEGACY_QUEUE_SEND_COUNT: AtomicU32 = AtomicU32::new(0);
static COMMON_LEGACY_QUEUE_SEND_FRONT_COUNT: AtomicU32 = AtomicU32::new(0);
static COMMON_LEGACY_QUEUE_RECV_COUNT: AtomicU32 = AtomicU32::new(0);
static COMMON_LEGACY_QUEUE_SEND_ISR_COUNT: AtomicU32 = AtomicU32::new(0);
static COMMON_LEGACY_QUEUE_RECV_ISR_COUNT: AtomicU32 = AtomicU32::new(0);
static COMMON_LEGACY_QUEUE_LAST_PTR: AtomicUsize = AtomicUsize::new(0);
static COMMON_LEGACY_QUEUE_SEND_RECENT_ORDINALS: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];
static COMMON_LEGACY_QUEUE_SEND_RECENT_ITEM_WORD0: [AtomicU32; 8] =
    [const { AtomicU32::new(0) }; 8];
static COMMON_LEGACY_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD0: [AtomicU32; 8] =
    [const { AtomicU32::new(0) }; 8];
static COMMON_LEGACY_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD1: [AtomicU32; 8] =
    [const { AtomicU32::new(0) }; 8];
static COMMON_LEGACY_QUEUE_RECV_RECENT_ORDINALS: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];
static COMMON_LEGACY_QUEUE_RECV_RECENT_ITEM_WORD0: [AtomicU32; 8] =
    [const { AtomicU32::new(0) }; 8];
static COMMON_LEGACY_QUEUE_RECV_RECENT_ITEM_POINTEE_WORD0: [AtomicU32; 8] =
    [const { AtomicU32::new(0) }; 8];
static COMMON_LEGACY_QUEUE_RECV_RECENT_ITEM_POINTEE_WORD1: [AtomicU32; 8] =
    [const { AtomicU32::new(0) }; 8];

fn record_recent_queue_item(
    ordinals: &[AtomicU32; 8],
    item_word0s: &[AtomicU32; 8],
    pointee_word0s: &[AtomicU32; 8],
    pointee_word1s: &[AtomicU32; 8],
    ordinal: u32,
    item: *const c_void,
    item_size: usize,
) {
    let slot = (ordinal as usize).wrapping_sub(1) % ordinals.len();
    let (item_word0, item_word1) = if item.is_null() || item_size == 0 {
        (0, 0)
    } else {
        let bytes = unsafe { core::slice::from_raw_parts(item.cast::<u8>(), item_size) };
        let word0 = if item_size >= 4 {
            u32::from_le_bytes(bytes[..4].try_into().unwrap())
        } else {
            0
        };
        let word1 = if item_size >= 8 {
            u32::from_le_bytes(bytes[4..8].try_into().unwrap())
        } else {
            0
        };
        (word0, word1)
    };
    let pointee_addr = item_word1 as usize;
    let pointee_is_plausible = item_word1 != 0 && (0x3ff0_0000..0x4000_0000).contains(&pointee_addr);
    let pointee = pointee_addr as *const u32;
    let pointee_word0 = if !pointee_is_plausible || pointee.is_null() {
        0
    } else {
        unsafe { pointee.read_unaligned() }
    };
    let pointee_word1 = if !pointee_is_plausible || pointee.is_null() {
        0
    } else {
        unsafe { pointee.add(1).read_unaligned() }
    };

    item_word0s[slot].store(item_word0, Ordering::Relaxed);
    pointee_word0s[slot].store(pointee_word0, Ordering::Relaxed);
    pointee_word1s[slot].store(pointee_word1, Ordering::Relaxed);
    ordinals[slot].store(ordinal, Ordering::Relaxed);
}

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
    pub(crate) fn new(count: usize, item_size: usize) -> Self {
        Self {
            raw_queue: Locked::new(RawQueue::new(count, item_size)),
        }
    }

    pub(crate) fn enqueue(&mut self, item: *const c_void) -> i32 {
        self.raw_queue.with(|q| unsafe { q.enqueue(item) })
    }

    pub(crate) fn enqueue_front(&mut self, item: *const c_void) -> i32 {
        self.raw_queue.with(|q| unsafe { q.enqueue_front(item) })
    }

    pub(crate) fn try_dequeue(&mut self, item: *mut c_void) -> bool {
        self.raw_queue.with(|q| unsafe { q.try_dequeue(item) })
    }

    pub(crate) fn remove(&mut self, item: *const c_void) {
        self.raw_queue.with(|q| unsafe { q.remove(item) })
    }

    pub(crate) fn count(&self) -> usize {
        self.raw_queue.with(|q| q.count())
    }
}

pub(crate) struct RawQueue {
    item_size: usize,
    capacity: usize,
    current_read: usize,
    current_write: usize,
    storage: Box<[u8], InternalMemory>,
}

impl RawQueue {
    pub(crate) fn new(capacity: usize, item_size: usize) -> Self {
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

        let item = unsafe { core::slice::from_raw_parts(item.cast::<u8>(), self.item_size) };
        self.get_mut(self.current_write).copy_from_slice(item);
        self.current_write = (self.current_write + 1) % self.capacity;
        1
    }

    unsafe fn enqueue_front(&mut self, item: *const c_void) -> i32 {
        if self.full() {
            return 0;
        }

        let item = unsafe { core::slice::from_raw_parts(item.cast::<u8>(), self.item_size) };
        self.current_read = (self.current_read + self.capacity - 1) % self.capacity;
        self.get_mut(self.current_read).copy_from_slice(item);
        1
    }

    unsafe fn try_dequeue(&mut self, item: *mut c_void) -> bool {
        if self.empty() {
            return false;
        }

        let item = unsafe { core::slice::from_raw_parts_mut(item.cast::<u8>(), self.item_size) };
        item.copy_from_slice(self.get(self.current_read));
        self.current_read = (self.current_read + 1) % self.capacity;
        true
    }

    unsafe fn remove(&mut self, item: *const c_void) {
        let item_slice = unsafe { core::slice::from_raw_parts(item.cast::<u8>(), self.item_size) };
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
}

fn queue_ptr(queue: *mut c_void) -> *mut ConcurrentQueue {
    queue.cast()
}

pub(crate) fn create_queue(queue_len: c_int, item_size: c_int) -> *mut ConcurrentQueue {
    let queue = ConcurrentQueue::new(queue_len as usize, item_size as usize);
    let ptr = unsafe { malloc(size_of_val(&queue)) as *mut ConcurrentQueue };
    unsafe {
        ptr.write(queue);
    }
    ptr
}

pub(crate) fn delete_queue(queue: *mut ConcurrentQueue) {
    unsafe {
        ptr::drop_in_place(queue);
        free(queue.cast());
    }
}

pub(crate) fn send_queued(queue: *mut ConcurrentQueue, item: *mut c_void, block_time_tick: u32) -> i32 {
    let ordinal = COMMON_LEGACY_QUEUE_SEND_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    COMMON_LEGACY_QUEUE_LAST_PTR.store(queue as usize, Ordering::Relaxed);
    record_recent_queue_item(
        &COMMON_LEGACY_QUEUE_SEND_RECENT_ORDINALS,
        &COMMON_LEGACY_QUEUE_SEND_RECENT_ITEM_WORD0,
        &COMMON_LEGACY_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD0,
        &COMMON_LEGACY_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD1,
        ordinal,
        item,
        unsafe { (*queue).raw_queue.with(|q| q.item_size) },
    );
    let forever = block_time_tick == OSI_FUNCS_TIME_BLOCKING;
    let timeout = block_time_tick as u64;
    let start = time::systimer_count();

    loop {
        let sent = unsafe { (*queue).enqueue(item) };
        if sent == 1 {
            return 1;
        }
        if !forever && time::elapsed_time_since(start) > timeout {
            return 0;
        }
        legacy_preempt::yield_task();
    }
}

pub(crate) fn send_queued_front(
    queue: *mut ConcurrentQueue,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    let ordinal = COMMON_LEGACY_QUEUE_SEND_FRONT_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    COMMON_LEGACY_QUEUE_LAST_PTR.store(queue as usize, Ordering::Relaxed);
    record_recent_queue_item(
        &COMMON_LEGACY_QUEUE_SEND_RECENT_ORDINALS,
        &COMMON_LEGACY_QUEUE_SEND_RECENT_ITEM_WORD0,
        &COMMON_LEGACY_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD0,
        &COMMON_LEGACY_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD1,
        ordinal,
        item,
        unsafe { (*queue).raw_queue.with(|q| q.item_size) },
    );
    let forever = block_time_tick == OSI_FUNCS_TIME_BLOCKING;
    let timeout = block_time_tick as u64;
    let start = time::systimer_count();

    loop {
        let sent = unsafe { (*queue).enqueue_front(item) };
        if sent == 1 {
            return 1;
        }
        if !forever && time::elapsed_time_since(start) > timeout {
            return 0;
        }
        legacy_preempt::yield_task();
    }
}

pub(crate) fn receive_queued(queue: *mut ConcurrentQueue, item: *mut c_void, block_time_tick: u32) -> i32 {
    let ordinal = COMMON_LEGACY_QUEUE_RECV_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    COMMON_LEGACY_QUEUE_LAST_PTR.store(queue as usize, Ordering::Relaxed);
    let forever = block_time_tick == OSI_FUNCS_TIME_BLOCKING;
    let timeout = block_time_tick as u64;
    let start = time::systimer_count();

    loop {
        if unsafe { (*queue).try_dequeue(item) } {
            record_recent_queue_item(
                &COMMON_LEGACY_QUEUE_RECV_RECENT_ORDINALS,
                &COMMON_LEGACY_QUEUE_RECV_RECENT_ITEM_WORD0,
                &COMMON_LEGACY_QUEUE_RECV_RECENT_ITEM_POINTEE_WORD0,
                &COMMON_LEGACY_QUEUE_RECV_RECENT_ITEM_POINTEE_WORD1,
                ordinal,
                item,
                unsafe { (*queue).raw_queue.with(|q| q.item_size) },
            );
            return 1;
        }
        if !forever && time::elapsed_time_since(start) > timeout {
            return -1;
        }
        legacy_preempt::yield_task();
    }
}

pub(crate) fn number_of_messages_in_queue(queue: *const ConcurrentQueue) -> u32 {
    unsafe { (*queue).count() as u32 }
}

pub(crate) fn try_send_queued_from_isr(
    queue: *mut ConcurrentQueue,
    item: *mut c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    let ordinal = COMMON_LEGACY_QUEUE_SEND_ISR_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    COMMON_LEGACY_QUEUE_LAST_PTR.store(queue as usize, Ordering::Relaxed);
    record_recent_queue_item(
        &COMMON_LEGACY_QUEUE_SEND_RECENT_ORDINALS,
        &COMMON_LEGACY_QUEUE_SEND_RECENT_ITEM_WORD0,
        &COMMON_LEGACY_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD0,
        &COMMON_LEGACY_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD1,
        ordinal,
        item,
        unsafe { (*queue).raw_queue.with(|q| q.item_size) },
    );
    let sent = unsafe { (*queue).enqueue(item) };
    if sent == 1 {
        unsafe {
            if let Some(waken) = higher_priority_task_waken.as_mut() {
                *waken = true;
            }
        }
    }
    sent
}

pub(crate) fn try_receive_queued_from_isr(
    queue: *mut ConcurrentQueue,
    item: *mut c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    let ordinal = COMMON_LEGACY_QUEUE_RECV_ISR_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    COMMON_LEGACY_QUEUE_LAST_PTR.store(queue as usize, Ordering::Relaxed);
    let received = unsafe { (*queue).try_dequeue(item) };
    if received {
        record_recent_queue_item(
            &COMMON_LEGACY_QUEUE_RECV_RECENT_ORDINALS,
            &COMMON_LEGACY_QUEUE_RECV_RECENT_ITEM_WORD0,
            &COMMON_LEGACY_QUEUE_RECV_RECENT_ITEM_POINTEE_WORD0,
            &COMMON_LEGACY_QUEUE_RECV_RECENT_ITEM_POINTEE_WORD1,
            ordinal,
            item,
            unsafe { (*queue).raw_queue.with(|q| q.item_size) },
        );
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

pub(crate) fn remove_queued(queue: *mut ConcurrentQueue, item: *mut c_void) {
    unsafe { (*queue).remove(item) };
    memory_fence();
}

pub(crate) fn common_legacy_queue_diag() -> CommonLegacyQueueDiag {
    CommonLegacyQueueDiag {
        send_count: COMMON_LEGACY_QUEUE_SEND_COUNT.load(Ordering::Relaxed),
        send_front_count: COMMON_LEGACY_QUEUE_SEND_FRONT_COUNT.load(Ordering::Relaxed),
        recv_count: COMMON_LEGACY_QUEUE_RECV_COUNT.load(Ordering::Relaxed),
        send_isr_count: COMMON_LEGACY_QUEUE_SEND_ISR_COUNT.load(Ordering::Relaxed),
        recv_isr_count: COMMON_LEGACY_QUEUE_RECV_ISR_COUNT.load(Ordering::Relaxed),
        last_queue_ptr: COMMON_LEGACY_QUEUE_LAST_PTR.load(Ordering::Relaxed),
        recent_send_ordinals: core::array::from_fn(|idx| {
            COMMON_LEGACY_QUEUE_SEND_RECENT_ORDINALS[idx].load(Ordering::Relaxed)
        }),
        recent_send_item_word0: core::array::from_fn(|idx| {
            COMMON_LEGACY_QUEUE_SEND_RECENT_ITEM_WORD0[idx].load(Ordering::Relaxed)
        }),
        recent_send_item_pointee_word0: core::array::from_fn(|idx| {
            COMMON_LEGACY_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD0[idx].load(Ordering::Relaxed)
        }),
        recent_send_item_pointee_word1: core::array::from_fn(|idx| {
            COMMON_LEGACY_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD1[idx].load(Ordering::Relaxed)
        }),
        recent_recv_ordinals: core::array::from_fn(|idx| {
            COMMON_LEGACY_QUEUE_RECV_RECENT_ORDINALS[idx].load(Ordering::Relaxed)
        }),
        recent_recv_item_word0: core::array::from_fn(|idx| {
            COMMON_LEGACY_QUEUE_RECV_RECENT_ITEM_WORD0[idx].load(Ordering::Relaxed)
        }),
        recent_recv_item_pointee_word0: core::array::from_fn(|idx| {
            COMMON_LEGACY_QUEUE_RECV_RECENT_ITEM_POINTEE_WORD0[idx].load(Ordering::Relaxed)
        }),
        recent_recv_item_pointee_word1: core::array::from_fn(|idx| {
            COMMON_LEGACY_QUEUE_RECV_RECENT_ITEM_POINTEE_WORD1[idx].load(Ordering::Relaxed)
        }),
    }
}
