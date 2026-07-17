use esp_wifi_sys::c_types::{c_int, c_void};

use crate::compat::{common_legacy, common_legacy_queue as queue_legacy};

pub(crate) type ConcurrentQueue = queue_legacy::ConcurrentQueue;

pub(crate) fn create_queue(queue_len: c_int, item_size: c_int) -> *mut ConcurrentQueue {
    queue_legacy::create_queue(queue_len, item_size)
}

pub(crate) fn delete_queue(queue: *mut ConcurrentQueue) {
    queue_legacy::delete_queue(queue)
}

pub(crate) fn send_queued(
    queue: *mut ConcurrentQueue,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    queue_legacy::send_queued(queue, item, block_time_tick)
}

pub(crate) fn send_queued_to_front(
    queue: *mut ConcurrentQueue,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    unsafe { common_legacy::queue_send_to_front(queue.cast(), item, block_time_tick) }
}

pub(crate) fn receive_queued(
    queue: *mut ConcurrentQueue,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    queue_legacy::receive_queued(queue, item, block_time_tick)
}

pub(crate) fn number_of_messages_in_queue(queue: *const ConcurrentQueue) -> u32 {
    queue_legacy::number_of_messages_in_queue(queue)
}

pub(crate) fn try_send_queued_from_isr(
    queue: *mut ConcurrentQueue,
    item: *mut c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    queue_legacy::try_send_queued_from_isr(queue, item, higher_priority_task_waken)
}

pub(crate) fn try_receive_queued_from_isr(
    queue: *mut ConcurrentQueue,
    item: *mut c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    queue_legacy::try_receive_queued_from_isr(queue, item, higher_priority_task_waken)
}
