#![allow(unused)]

use core::{
    ffi::{c_char, c_void},
    ptr::addr_of_mut,
};

use num_traits::FromPrimitive;

use crate::{
    compat::{common_legacy_literal, common_legacy_port, malloc},
    memory_fence::memory_fence,
    wifi::{self, os_adapter::WIFI_EVENTS, WifiEvent},
};

static mut WIFI_STATIC_QUEUE_HANDLE: *mut c_void = core::ptr::null_mut();

pub(crate) fn thread_sem_get() -> *mut c_void {
    common_legacy_literal::thread_sem_get()
}

pub(crate) unsafe extern "C" fn semphr_create(max: u32, init: u32) -> *mut c_void {
    common_legacy_literal::sem_create(max, init)
}

pub(crate) unsafe extern "C" fn semphr_delete(semphr: *mut c_void) {
    common_legacy_literal::sem_delete(semphr)
}

pub(crate) unsafe extern "C" fn semphr_take(semphr: *mut c_void, tick: u32) -> i32 {
    common_legacy_literal::sem_take(semphr, tick)
}

pub(crate) unsafe extern "C" fn semphr_take_from_isr(
    semphr: *mut c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    common_legacy_literal::sem_take_from_isr(semphr, higher_priority_task_waken)
}

pub(crate) unsafe extern "C" fn semphr_give(semphr: *mut c_void) -> i32 {
    common_legacy_literal::sem_give(semphr)
}

pub(crate) unsafe extern "C" fn semphr_give_from_isr(
    semphr: *mut c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    common_legacy_literal::sem_give_from_isr(semphr, higher_priority_task_waken)
}

pub(crate) unsafe extern "C" fn queue_send_from_isr(
    queue: *mut c_void,
    item: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    common_legacy_literal::try_send_queued_from_isr(
        queue.cast(),
        item,
        higher_priority_task_waken.cast(),
    )
}

pub(crate) unsafe extern "C" fn queue_send_to_back(
    queue: *mut c_void,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    common_legacy_literal::send_queued(queue.cast(), item, block_time_tick)
}

pub(crate) unsafe extern "C" fn queue_send_to_front(
    _queue: *mut c_void,
    _item: *mut c_void,
    _block_time_tick: u32,
) -> i32 {
    common_legacy_literal::send_queued_front(_queue.cast(), _item, _block_time_tick)
}

pub(crate) unsafe extern "C" fn queue_recv(
    queue: *mut c_void,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    common_legacy_literal::receive_queued(queue.cast(), item, block_time_tick)
}

pub(crate) unsafe extern "C" fn queue_recv_from_isr(
    queue: *mut c_void,
    item: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    common_legacy_literal::try_receive_queued_from_isr(
        queue.cast(),
        item,
        higher_priority_task_waken.cast(),
    )
}

pub(crate) unsafe extern "C" fn queue_msg_waiting(queue: *mut c_void) -> u32 {
    common_legacy_literal::number_of_messages_in_queue(queue.cast())
}

pub(crate) unsafe extern "C" fn queue_create(queue_len: u32, item_size: u32) -> *mut c_void {
    common_legacy_literal::create_queue(queue_len as i32, item_size as i32).cast()
}

pub(crate) unsafe extern "C" fn queue_delete(queue: *mut c_void) {
    common_legacy_literal::delete_queue(queue.cast())
}

pub(crate) unsafe extern "C" fn event_group_create() -> *mut c_void {
    unsafe { common_legacy_port::event_group_create() }
}

pub(crate) unsafe extern "C" fn event_group_delete(event: *mut c_void) {
    unsafe { common_legacy_port::event_group_delete(event) }
}

pub(crate) unsafe extern "C" fn event_group_set_bits(event: *mut c_void, bits: u32) -> u32 {
    unsafe { common_legacy_port::event_group_set_bits(event, bits) }
}

pub(crate) unsafe extern "C" fn event_group_clear_bits(event: *mut c_void, bits: u32) -> u32 {
    unsafe { common_legacy_port::event_group_clear_bits(event, bits) }
}

pub(crate) unsafe extern "C" fn event_group_wait_bits(
    event: *mut c_void,
    bits_to_wait_for: u32,
    clear_on_exit: i32,
    wait_for_all_bits: i32,
    block_time_tick: u32,
) -> u32 {
    unsafe {
        common_legacy_port::event_group_wait_bits(
            event,
            bits_to_wait_for,
            clear_on_exit,
            wait_for_all_bits,
            block_time_tick,
        )
    }
}

pub(crate) unsafe extern "C" fn mutex_create(recursive: bool) -> *mut c_void {
    if recursive {
        common_legacy_literal::create_recursive_mutex()
    } else {
        common_legacy_literal::create_mutex()
    }
}

pub(crate) unsafe extern "C" fn mutex_delete(mutex: *mut c_void) {
    common_legacy_literal::mutex_delete(mutex)
}

pub(crate) unsafe extern "C" fn mutex_lock(mutex: *mut c_void) -> i32 {
    common_legacy_literal::lock_mutex(mutex)
}

pub(crate) unsafe extern "C" fn mutex_unlock(mutex: *mut c_void) -> i32 {
    common_legacy_literal::unlock_mutex(mutex)
}

pub(crate) unsafe extern "C" fn malloc_internal(size: usize) -> *mut c_void {
    unsafe { malloc::malloc(size).cast() }
}

pub(crate) unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    unsafe { malloc::malloc(size).cast() }
}

pub(crate) unsafe extern "C" fn free(ptr: *mut c_void) {
    unsafe { malloc::free(ptr.cast()) }
}

pub(crate) unsafe extern "C" fn realloc_internal(ptr: *mut c_void, size: usize) -> *mut c_void {
    unsafe { malloc::realloc_internal(ptr.cast(), size).cast() }
}

pub(crate) unsafe extern "C" fn calloc_internal(n: usize, size: usize) -> *mut c_void {
    unsafe { malloc::calloc(n as u32, size).cast() }
}

pub(crate) unsafe extern "C" fn zalloc_internal(size: usize) -> *mut c_void {
    unsafe { malloc::calloc(size as u32, 1usize).cast() }
}

pub(crate) unsafe extern "C" fn wifi_malloc(size: usize) -> *mut c_void {
    unsafe { malloc::malloc(size).cast() }
}

pub(crate) unsafe extern "C" fn wifi_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    unsafe { realloc_internal(ptr, size) }
}

pub(crate) unsafe extern "C" fn wifi_calloc(n: usize, size: usize) -> *mut c_void {
    unsafe { malloc::calloc(n as u32, size).cast() }
}

pub(crate) unsafe extern "C" fn wifi_zalloc(size: usize) -> *mut c_void {
    unsafe { wifi_calloc(size, 1) }
}

pub(crate) unsafe extern "C" fn wifi_create_queue(queue_len: i32, item_size: i32) -> *mut c_void {
    unsafe {
        let queue = common_legacy_literal::create_queue(queue_len, item_size).cast();
        WIFI_STATIC_QUEUE_HANDLE = queue;
        addr_of_mut!(WIFI_STATIC_QUEUE_HANDLE).cast()
    }
}

pub(crate) unsafe extern "C" fn wifi_delete_queue(queue: *mut c_void) {
    unsafe {
        if core::ptr::eq(queue, addr_of_mut!(WIFI_STATIC_QUEUE_HANDLE).cast()) {
            common_legacy_literal::delete_queue(WIFI_STATIC_QUEUE_HANDLE.cast());
            WIFI_STATIC_QUEUE_HANDLE = core::ptr::null_mut();
        }
    }
}

pub(crate) unsafe extern "C" fn esp_timer_get_time() -> i64 {
    unsafe { common_legacy_port::esp_timer_get_time() }
}

pub(crate) unsafe extern "C" fn vtask_delay(ticks: u32) {
    common_legacy_literal::task_delay(ticks)
}

pub(crate) unsafe extern "C" fn event_post(
    _event_base: *const c_char,
    event_id: i32,
    event_data: *mut c_void,
    event_data_size: usize,
    _ticks_to_wait: u32,
) -> i32 {
    let event = WifiEvent::from_i32(event_id).expect("invalid wifi event id");
    WIFI_EVENTS.with(|events| events.insert(event));

    let handled = unsafe { wifi::event::dispatch_event_handler(event, event_data, event_data_size) };
    wifi::state::update_state(event, handled);
    event.waker().wake();

    match event {
        WifiEvent::StaConnected | WifiEvent::StaDisconnected => {
            wifi::embassy::STA_LINK_STATE_WAKER.wake();
        }
        WifiEvent::ApStart | WifiEvent::ApStop => {
            wifi::embassy::AP_LINK_STATE_WAKER.wake();
        }
        _ => {}
    }

    memory_fence();
    0
}

pub(crate) unsafe extern "C" fn get_free_heap_size() -> u32 {
    unsafe { crate::compat::malloc::get_free_internal_heap_size() as u32 }
}

pub(crate) unsafe extern "C" fn rand() -> u32 {
    unsafe { crate::common_adapter::random() as u32 }
}

pub(crate) unsafe extern "C" fn get_random(buf: *mut u8, len: usize) -> i32 {
    unsafe {
        crate::common_adapter::__esp_radio_esp_fill_random(buf, len as u32);
    }
    0
}

pub(crate) unsafe extern "C" fn get_time(_t: *mut c_void) -> i32 {
    todo!("get_time")
}

pub(crate) unsafe extern "C" fn random() -> u32 {
    unsafe { crate::common_adapter::random() as u32 }
}

pub(crate) unsafe extern "C" fn log_timestamp() -> u32 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_millis() as u32
}
