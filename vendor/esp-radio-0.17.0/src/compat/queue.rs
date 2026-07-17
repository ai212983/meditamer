use esp_wifi_sys::c_types::*;

use crate::{
    compat::{
        OSI_FUNCS_TIME_BLOCKING,
        queue_legacy,
    },
    preempt::queue::{QueueHandle, QueuePtr},
};

fn legacy_simple_queue_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_LEGACY_SIMPLE_QUEUE_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

pub(crate) fn queue_create(queue_len: c_int, item_size: c_int) -> *mut c_void {
    trace!("queue_create len={} size={}", queue_len, item_size);
    if legacy_simple_queue_enabled() {
        return queue_legacy::queue_create(queue_len, item_size);
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
        return queue_legacy::queue_delete(queue);
    }
    let ptr = unwrap!(QueuePtr::new(queue.cast()), "queue is null");
    let handle = unsafe { QueueHandle::from_ptr(ptr) };
    core::mem::drop(handle);
}

pub(crate) fn queue_send_to_back(queue: *mut c_void, item: *const c_void, tick: u32) -> i32 {
    trace!("queue_send queue {:?} item {:x} tick {}", queue, item as usize, tick);
    if legacy_simple_queue_enabled() {
        return queue_legacy::queue_send_to_back(queue, item, tick);
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
        return queue_legacy::queue_try_send_to_back_from_isr(
            queue,
            item,
            higher_priority_task_waken,
        );
    }
    let ptr = unwrap!(QueuePtr::new(queue.cast()), "queue is null");
    let handle = unsafe { QueueHandle::ref_from_ptr(&ptr) };
    unsafe { handle.try_send_to_back_from_isr(item.cast(), higher_priority_task_waken.as_mut()) as i32 }
}

pub(crate) fn queue_send_to_front(queue: *mut c_void, item: *const c_void, tick: u32) -> i32 {
    trace!("queue_send_to_front {:?} item {:?} tick {}", queue, item, tick);
    if legacy_simple_queue_enabled() {
        return queue_legacy::queue_send_to_front(queue, item, tick);
    }
    let ptr = unwrap!(QueuePtr::new(queue.cast()), "queue is null");
    let handle = unsafe { QueueHandle::ref_from_ptr(&ptr) };
    let timeout = if tick == OSI_FUNCS_TIME_BLOCKING { None } else { Some(tick) };
    unsafe { handle.send_to_front(item.cast(), timeout) as i32 }
}

pub(crate) fn queue_receive(queue: *mut c_void, item: *mut c_void, tick: u32) -> i32 {
    trace!("queue_recv {:?} item {:?} tick {}", queue, item, tick);
    if legacy_simple_queue_enabled() {
        return queue_legacy::queue_receive(queue, item, tick);
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
        return queue_legacy::queue_try_receive_from_isr(queue, item, higher_priority_task_waken);
    }
    let ptr = unwrap!(QueuePtr::new(queue.cast()), "queue is null");
    let handle = unsafe { QueueHandle::ref_from_ptr(&ptr) };
    unsafe { handle.try_receive_from_isr(item.cast(), higher_priority_task_waken.as_mut()) as i32 }
}

pub(crate) fn queue_remove(queue: *mut c_void, item: *const c_void) {
    trace!("queue_remove queue {:?} item {:x}", queue, item as usize);
    if legacy_simple_queue_enabled() {
        return queue_legacy::queue_remove(queue, item);
    }
    let ptr = unwrap!(QueuePtr::new(queue.cast()), "queue is null");
    let handle = unsafe { QueueHandle::ref_from_ptr(&ptr) };
    unsafe { handle.remove(item.cast()) }
}

pub(crate) fn queue_messages_waiting(queue: *mut c_void) -> u32 {
    trace!("queue_msg_waiting {:?}", queue);
    if legacy_simple_queue_enabled() {
        return queue_legacy::queue_messages_waiting(queue);
    }
    let ptr = unwrap!(QueuePtr::new(queue.cast()), "queue is null");
    let handle = unsafe { QueueHandle::ref_from_ptr(&ptr) };
    handle.messages_waiting() as u32
}
