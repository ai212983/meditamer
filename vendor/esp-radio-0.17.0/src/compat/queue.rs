use alloc::{boxed::Box, vec};

use esp_hal::time::{Duration, Instant};
use esp_wifi_sys::c_types::*;

use crate::{
    ESP_RADIO_LOCK,
    compat::OSI_FUNCS_TIME_BLOCKING,
    preempt::queue::{QueueHandle, QueuePtr},
};

fn legacy_simple_queue_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_LEGACY_SIMPLE_QUEUE_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

struct LegacyQueue {
    item_size: usize,
    capacity: usize,
    count: usize,
    current_read: usize,
    current_write: usize,
    storage: Box<[u8]>,
}

impl LegacyQueue {
    fn new(capacity: usize, item_size: usize) -> Self {
        Self {
            item_size,
            capacity,
            count: 0,
            current_read: 0,
            current_write: 0,
            storage: vec![0; capacity * item_size].into_boxed_slice(),
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

    fn full(&self) -> bool {
        self.count == self.capacity
    }

    fn empty(&self) -> bool {
        self.count == 0
    }

    unsafe fn try_enqueue_back(&mut self, item: *const u8) -> bool {
        if self.full() {
            return false;
        }
        let src = unsafe { core::slice::from_raw_parts(item, self.item_size) };
        self.get_mut(self.current_write).copy_from_slice(src);
        self.current_write = (self.current_write + 1) % self.capacity;
        self.count += 1;
        true
    }

    unsafe fn try_enqueue_front(&mut self, item: *const u8) -> bool {
        if self.full() {
            return false;
        }
        let src = unsafe { core::slice::from_raw_parts(item, self.item_size) };
        self.current_read = (self.current_read + self.capacity - 1) % self.capacity;
        self.get_mut(self.current_read).copy_from_slice(src);
        self.count += 1;
        true
    }

    unsafe fn try_dequeue(&mut self, item: *mut u8) -> bool {
        if self.empty() {
            return false;
        }
        let dst = unsafe { core::slice::from_raw_parts_mut(item, self.item_size) };
        dst.copy_from_slice(self.get(self.current_read));
        self.current_read = (self.current_read + 1) % self.capacity;
        self.count -= 1;
        true
    }

    unsafe fn remove(&mut self, item: *const u8) {
        if self.empty() {
            return;
        }
        let count = self.count;
        let target = unsafe { core::slice::from_raw_parts(item, self.item_size) };
        let mut tmp = vec![0; self.item_size];
        for _ in 0..count {
            if !unsafe { self.try_dequeue(tmp.as_mut_ptr()) } {
                break;
            }
            if &tmp[..] != target {
                let _ = unsafe { self.try_enqueue_back(tmp.as_ptr()) };
            }
        }
    }
}

fn legacy_queue_ptr(queue: *mut c_void) -> *mut LegacyQueue {
    queue.cast()
}

fn legacy_queue_timeout_deadline(tick: u32) -> Option<Instant> {
    if tick == OSI_FUNCS_TIME_BLOCKING {
        None
    } else {
        Some(Instant::now() + Duration::from_micros(tick as u64))
    }
}

pub(crate) fn queue_create(queue_len: c_int, item_size: c_int) -> *mut c_void {
    trace!("queue_create len={} size={}", queue_len, item_size);
    if legacy_simple_queue_enabled() {
        let queue = Box::new(LegacyQueue::new(queue_len as usize, item_size as usize));
        let ptr = Box::into_raw(queue).cast();
        trace!("created legacy queue @{:?}", ptr);
        return ptr;
    }
    let queue = QueueHandle::new(queue_len as usize, item_size as usize)
        .leak()
        .as_ptr()
        .cast();
    trace!("created queue @{:?}", queue);
    queue
}

pub(crate) fn queue_delete(queue: *mut c_void) {
    trace!("delete_queue {:?}", queue);
    if legacy_simple_queue_enabled() {
        unsafe { drop(Box::from_raw(legacy_queue_ptr(queue))) };
        return;
    }
    let ptr = unwrap!(QueuePtr::new(queue.cast()), "queue is null");
    let handle = unsafe { QueueHandle::from_ptr(ptr) };
    core::mem::drop(handle);
}

pub(crate) fn queue_send_to_back(queue: *mut c_void, item: *const c_void, tick: u32) -> i32 {
    trace!("queue_send queue {:?} item {:x} tick {}", queue, item as usize, tick);
    if legacy_simple_queue_enabled() {
        let queue = legacy_queue_ptr(queue);
        let deadline = legacy_queue_timeout_deadline(tick);
        loop {
            let sent = ESP_RADIO_LOCK.lock(|| unsafe { (*queue).try_enqueue_back(item.cast()) });
            if sent {
                return 1;
            }
            if deadline.is_some_and(|d| d < Instant::now()) {
                return 0;
            }
            crate::preempt::yield_task();
        }
    }
    let ptr = unwrap!(QueuePtr::new(queue.cast()), "queue is null");
    let handle = unsafe { QueueHandle::ref_from_ptr(&ptr) };
    let timeout = if tick == OSI_FUNCS_TIME_BLOCKING { None } else { Some(tick) };
    unsafe { handle.send_to_back(item.cast(), timeout) as i32 }
}

pub(crate) fn queue_try_send_to_back_from_isr(
    queue: *mut c_void,
    item: *const c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    trace!("queue_try_send_to_back_from_isr queue {:?} item {:x}", queue, item as usize);
    if legacy_simple_queue_enabled() {
        let sent = ESP_RADIO_LOCK.lock(|| unsafe { (*legacy_queue_ptr(queue)).try_enqueue_back(item.cast()) });
        if sent {
            unsafe {
                if let Some(waken) = higher_priority_task_waken.as_mut() {
                    *waken = true;
                }
            }
            return 1;
        }
        return 0;
    }
    let ptr = unwrap!(QueuePtr::new(queue.cast()), "queue is null");
    let handle = unsafe { QueueHandle::ref_from_ptr(&ptr) };
    unsafe { handle.try_send_to_back_from_isr(item.cast(), higher_priority_task_waken.as_mut()) as i32 }
}

pub(crate) fn queue_send_to_front(queue: *mut c_void, item: *const c_void, tick: u32) -> i32 {
    trace!("queue_send_to_front {:?} item {:?} tick {}", queue, item, tick);
    if legacy_simple_queue_enabled() {
        let queue = legacy_queue_ptr(queue);
        let deadline = legacy_queue_timeout_deadline(tick);
        loop {
            let sent = ESP_RADIO_LOCK.lock(|| unsafe { (*queue).try_enqueue_front(item.cast()) });
            if sent {
                return 1;
            }
            if deadline.is_some_and(|d| d < Instant::now()) {
                return 0;
            }
            crate::preempt::yield_task();
        }
    }
    let ptr = unwrap!(QueuePtr::new(queue.cast()), "queue is null");
    let handle = unsafe { QueueHandle::ref_from_ptr(&ptr) };
    let timeout = if tick == OSI_FUNCS_TIME_BLOCKING { None } else { Some(tick) };
    unsafe { handle.send_to_front(item.cast(), timeout) as i32 }
}

pub(crate) fn queue_receive(queue: *mut c_void, item: *mut c_void, tick: u32) -> i32 {
    trace!("queue_recv {:?} item {:?} tick {}", queue, item, tick);
    if legacy_simple_queue_enabled() {
        let queue = legacy_queue_ptr(queue);
        let deadline = legacy_queue_timeout_deadline(tick);
        loop {
            let received = ESP_RADIO_LOCK.lock(|| unsafe { (*queue).try_dequeue(item.cast()) });
            if received {
                return 1;
            }
            if deadline.is_some_and(|d| d < Instant::now()) {
                return -1;
            }
            crate::preempt::yield_task();
        }
    }
    let ptr = unwrap!(QueuePtr::new(queue.cast()), "queue is null");
    let handle = unsafe { QueueHandle::ref_from_ptr(&ptr) };
    let timeout = if tick == OSI_FUNCS_TIME_BLOCKING { None } else { Some(tick) };
    unsafe { handle.receive(item.cast(), timeout) as i32 }
}

pub(crate) fn queue_try_receive_from_isr(
    queue: *mut c_void,
    item: *mut c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    trace!("queue_try_recv_from_isr {:?} item {:?}", queue, item);
    if legacy_simple_queue_enabled() {
        let received = ESP_RADIO_LOCK.lock(|| unsafe { (*legacy_queue_ptr(queue)).try_dequeue(item.cast()) });
        if received {
            unsafe {
                if let Some(waken) = higher_priority_task_waken.as_mut() {
                    *waken = true;
                }
            }
            return 1;
        }
        return 0;
    }
    let ptr = unwrap!(QueuePtr::new(queue.cast()), "queue is null");
    let handle = unsafe { QueueHandle::ref_from_ptr(&ptr) };
    unsafe { handle.try_receive_from_isr(item.cast(), higher_priority_task_waken.as_mut()) as i32 }
}

pub(crate) fn queue_remove(queue: *mut c_void, item: *const c_void) {
    trace!("queue_remove queue {:?} item {:x}", queue, item as usize);
    if legacy_simple_queue_enabled() {
        ESP_RADIO_LOCK.lock(|| unsafe { (*legacy_queue_ptr(queue)).remove(item.cast()) });
        return;
    }
    let ptr = unwrap!(QueuePtr::new(queue.cast()), "queue is null");
    let handle = unsafe { QueueHandle::ref_from_ptr(&ptr) };
    unsafe { handle.remove(item.cast()) }
}

pub(crate) fn queue_messages_waiting(queue: *mut c_void) -> u32 {
    trace!("queue_msg_waiting {:?}", queue);
    if legacy_simple_queue_enabled() {
        return ESP_RADIO_LOCK.lock(|| unsafe { (*legacy_queue_ptr(queue)).count as u32 });
    }
    let ptr = unwrap!(QueuePtr::new(queue.cast()), "queue is null");
    let handle = unsafe { QueueHandle::ref_from_ptr(&ptr) };
    handle.messages_waiting() as u32
}
