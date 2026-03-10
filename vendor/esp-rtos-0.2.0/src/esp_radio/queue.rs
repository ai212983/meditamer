use alloc::{boxed::Box, vec};
use core::ptr::NonNull;

use esp_radio_rtos_driver::{
    queue::{QueueImplementation, QueuePtr},
    register_queue_implementation,
};
use esp_sync::NonReentrantMutex;
use portable_atomic::{AtomicU32, Ordering};

use crate::wait_queue::WaitQueue;

fn legacy_poll_wait_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RTOS_LEGACY_POLL_WAIT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn create_trace_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RTOS_CREATE_TRACE_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

static QUEUE_CREATE_COUNT: AtomicU32 = AtomicU32::new(0);
static QUEUE_CREATE_LAST_CAPACITY: AtomicU32 = AtomicU32::new(0);
static QUEUE_CREATE_LAST_ITEM_SIZE: AtomicU32 = AtomicU32::new(0);

struct QueueInner {
    storage: Box<[u8]>,
    item_size: usize,
    capacity: usize,
    count: usize,
    current_read: usize,
    current_write: usize,
    waiting_for_space: WaitQueue,
    waiting_for_item: WaitQueue,
}

impl QueueInner {
    fn new(capacity: usize, item_size: usize) -> Self {
        Self {
            item_size,
            capacity,
            count: 0,
            current_read: 0,
            current_write: 0,
            storage: vec![0; capacity * item_size].into_boxed_slice(),
            waiting_for_space: WaitQueue::new(),
            waiting_for_item: WaitQueue::new(),
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

    unsafe fn try_enqueue(&mut self, item: *const u8) -> bool {
        if self.full() {
            return false;
        }

        let item = unsafe { core::slice::from_raw_parts(item, self.item_size) };

        let dst = self.get_mut(self.current_write);
        dst.copy_from_slice(item);

        self.current_write = (self.current_write + 1) % self.capacity;
        self.count += 1;

        true
    }

    unsafe fn try_enqueue_front(&mut self, item: *const u8) -> bool {
        if self.full() {
            return false;
        }

        let item = unsafe { core::slice::from_raw_parts(item, self.item_size) };

        self.current_read = (self.current_read + self.capacity - 1) % self.capacity;
        let dst = self.get_mut(self.current_read);
        dst.copy_from_slice(item);

        self.count += 1;

        true
    }

    unsafe fn try_dequeue(&mut self, dst: *mut u8) -> bool {
        if self.empty() {
            return false;
        }

        let dst = unsafe { core::slice::from_raw_parts_mut(dst, self.item_size) };

        let src = self.get(self.current_read);
        dst.copy_from_slice(src);

        self.current_read = (self.current_read + 1) % self.capacity;
        self.count -= 1;

        true
    }

    unsafe fn remove(&mut self, item: *const u8) {
        if self.empty() {
            return;
        }

        // do what the ESP-IDF implementations does...
        // just remove all elements and add them back except the one we need to remove -
        // good enough for now
        let count = self.len();

        let mut tmp_item = vec![0; self.item_size];

        let item_slice = unsafe { core::slice::from_raw_parts(item, self.item_size) };
        for _ in 0..count {
            if !unsafe { self.try_dequeue(tmp_item.as_mut_ptr().cast()) } {
                break;
            }
            if &tmp_item[..] != item_slice {
                _ = unsafe { self.try_enqueue(tmp_item.as_mut_ptr().cast()) };
            }
            // Note that even if we find our item, we'll need to keep cycling through everything to
            // keep insertion order.
        }
    }

    fn len(&self) -> usize {
        self.count
    }

    fn empty(&self) -> bool {
        self.len() == 0
    }

    fn full(&self) -> bool {
        self.len() == self.capacity
    }
}

pub struct Queue {
    inner: NonReentrantMutex<QueueInner>,
}

impl Queue {
    pub fn new(capacity: usize, item_size: usize) -> Self {
        Queue {
            inner: NonReentrantMutex::new(QueueInner::new(capacity, item_size)),
        }
    }

    unsafe fn from_ptr<'a>(ptr: QueuePtr) -> &'a Self {
        unsafe { ptr.cast::<Self>().as_ref() }
    }

    fn item_size(&self) -> usize {
        self.inner.with(|queue| queue.item_size)
    }

    unsafe fn send_to_back(&self, item: *const u8, timeout_us: Option<u32>) -> bool {
        if legacy_poll_wait_enabled() {
            let deadline = timeout_us
                .map(|us| esp_hal::time::Instant::now() + esp_hal::time::Duration::from_micros(us as u64))
                .unwrap_or(esp_hal::time::Instant::EPOCH + esp_hal::time::Duration::MAX);
            loop {
                if self.inner.with(|queue| unsafe { queue.try_enqueue(item) }) {
                    self.inner.with(|queue| queue.waiting_for_item.notify());
                    return true;
                }
                if timeout_us.is_some() && deadline < esp_hal::time::Instant::now() {
                    return false;
                }
                crate::task::yield_task();
            }
        }
        if crate::with_deadline(timeout_us, |deadline| {
            self.inner.with(|queue| {
                if unsafe { queue.try_enqueue(item) } {
                    trace!("Queue - notify with item");
                    queue.waiting_for_item.notify();
                    true
                } else {
                    // The task will go to sleep when the above critical section is released.
                    trace!("Queue - wait for space - {:?}", deadline);
                    queue.waiting_for_space.wait_with_deadline(deadline);
                    false
                }
            })
        }) {
            debug!("Queue - send to back - success");
            true
        } else {
            debug!("Queue - send to back - timed out");
            false
        }
    }

    unsafe fn send_to_front(&self, item: *const u8, timeout_us: Option<u32>) -> bool {
        if legacy_poll_wait_enabled() {
            let deadline = timeout_us
                .map(|us| esp_hal::time::Instant::now() + esp_hal::time::Duration::from_micros(us as u64))
                .unwrap_or(esp_hal::time::Instant::EPOCH + esp_hal::time::Duration::MAX);
            loop {
                if self.inner.with(|queue| unsafe { queue.try_enqueue_front(item) }) {
                    self.inner.with(|queue| queue.waiting_for_item.notify());
                    return true;
                }
                if timeout_us.is_some() && deadline < esp_hal::time::Instant::now() {
                    return false;
                }
                crate::task::yield_task();
            }
        }
        if crate::with_deadline(timeout_us, |deadline| {
            self.inner.with(|queue| {
                if unsafe { queue.try_enqueue_front(item) } {
                    trace!("Queue - notify with item");
                    queue.waiting_for_item.notify();
                    true
                } else {
                    // The task will go to sleep when the above critical section is released.
                    trace!("Queue - wait for space - {:?}", deadline);
                    queue.waiting_for_space.wait_with_deadline(deadline);
                    false
                }
            })
        }) {
            debug!("Queue - send to front - success");
            true
        } else {
            debug!("Queue - send to front - timed out");
            false
        }
    }

    unsafe fn try_send_to_back(&self, item: *const u8) -> bool {
        self.inner.with(|queue| {
            if unsafe { queue.try_enqueue(item) } {
                queue.waiting_for_item.notify();
                true
            } else {
                false
            }
        })
    }

    unsafe fn receive(&self, item: *mut u8, timeout_us: Option<u32>) -> bool {
        if legacy_poll_wait_enabled() {
            let deadline = timeout_us
                .map(|us| esp_hal::time::Instant::now() + esp_hal::time::Duration::from_micros(us as u64))
                .unwrap_or(esp_hal::time::Instant::EPOCH + esp_hal::time::Duration::MAX);
            loop {
                if self.inner.with(|queue| unsafe { queue.try_dequeue(item) }) {
                    self.inner.with(|queue| queue.waiting_for_space.notify());
                    return true;
                }
                if timeout_us.is_some() && deadline < esp_hal::time::Instant::now() {
                    return false;
                }
                crate::task::yield_task();
            }
        }
        if crate::with_deadline(timeout_us, |deadline| {
            self.inner.with(|queue| {
                if unsafe { queue.try_dequeue(item) } {
                    trace!("Queue - notify with space");
                    queue.waiting_for_space.notify();
                    true
                } else {
                    // The task will go to sleep when the above critical section is released.
                    trace!("Queue - wait for item - {:?}", deadline);
                    queue.waiting_for_item.wait_with_deadline(deadline);
                    false
                }
            })
        }) {
            debug!("Queue - dequeued item");
            true
        } else {
            debug!("Queue - timed out waiting for item");
            false
        }
    }

    unsafe fn try_receive(&self, item: *mut u8) -> bool {
        self.inner.with(|queue| {
            if unsafe { queue.try_dequeue(item) } {
                trace!("Queue - notify with space");
                queue.waiting_for_space.notify();
                true
            } else {
                false
            }
        })
    }

    unsafe fn remove(&self, item: *const u8) {
        self.inner.with(|queue| {
            let was_full = queue.full();

            unsafe {
                queue.remove(item);
            }

            if was_full && !queue.full() {
                trace!("Queue - notify with space");
                queue.waiting_for_space.notify();
            }
        })
    }

    fn messages_waiting(&self) -> usize {
        self.inner.with(|queue| queue.len())
    }
}

impl QueueImplementation for Queue {
    fn create(capacity: usize, item_size: usize) -> QueuePtr {
        QUEUE_CREATE_COUNT.fetch_add(1, Ordering::Relaxed);
        QUEUE_CREATE_LAST_CAPACITY.store(capacity as u32, Ordering::Relaxed);
        QUEUE_CREATE_LAST_ITEM_SIZE.store(item_size as u32, Ordering::Relaxed);
        let q = Box::new(Queue::new(capacity, item_size));
        NonNull::from(Box::leak(q)).cast()
    }

    unsafe fn delete(queue: QueuePtr) {
        let q = unsafe { Box::from_raw(queue.cast::<Queue>().as_ptr()) };
        core::mem::drop(q);
    }

    unsafe fn send_to_front(queue: QueuePtr, item: *const u8, timeout_us: Option<u32>) -> bool {
        let queue = unsafe { Queue::from_ptr(queue) };

        unsafe { queue.send_to_front(item, timeout_us) }
    }

    unsafe fn send_to_back(queue: QueuePtr, item: *const u8, timeout_us: Option<u32>) -> bool {
        let queue = unsafe { Queue::from_ptr(queue) };

        unsafe { queue.send_to_back(item, timeout_us) }
    }

    unsafe fn try_send_to_back_from_isr(
        queue: QueuePtr,
        item: *const u8,
        _higher_prio_task_waken: Option<&mut bool>,
    ) -> bool {
        let queue = unsafe { Queue::from_ptr(queue) };

        unsafe { queue.try_send_to_back(item) }
    }

    unsafe fn receive(queue: QueuePtr, item: *mut u8, timeout_us: Option<u32>) -> bool {
        let queue = unsafe { Queue::from_ptr(queue) };

        unsafe { queue.receive(item, timeout_us) }
    }

    unsafe fn try_receive_from_isr(
        queue: QueuePtr,
        item: *mut u8,
        _higher_prio_task_waken: Option<&mut bool>,
    ) -> bool {
        let queue = unsafe { Queue::from_ptr(queue) };

        unsafe { queue.try_receive(item) }
    }

    unsafe fn remove(queue: QueuePtr, item: *const u8) {
        let queue = unsafe { Queue::from_ptr(queue) };

        unsafe { queue.remove(item) }
    }

    fn messages_waiting(queue: QueuePtr) -> usize {
        let queue = unsafe { Queue::from_ptr(queue) };

        queue.messages_waiting()
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn esp_rtos_queue_item_size(queue: QueuePtr) -> usize {
    let queue = unsafe { Queue::from_ptr(queue) };
    queue.item_size()
}

register_queue_implementation!(Queue);

pub(crate) fn queue_create_count() -> u32 {
    QUEUE_CREATE_COUNT.load(Ordering::Relaxed)
}

pub(crate) fn queue_create_last_capacity() -> u32 {
    QUEUE_CREATE_LAST_CAPACITY.load(Ordering::Relaxed)
}

pub(crate) fn queue_create_last_item_size() -> u32 {
    QUEUE_CREATE_LAST_ITEM_SIZE.load(Ordering::Relaxed)
}
