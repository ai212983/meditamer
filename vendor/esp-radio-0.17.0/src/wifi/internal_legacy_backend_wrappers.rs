use core::ffi::c_void;

use esp_wifi_sys::c_types::c_char;

use super::internal_legacy_common_literal as legacy_osi;

pub(crate) unsafe extern "C" fn task_yield_from_isr() {
    unsafe { legacy_osi::task_yield_from_isr() }
}

pub(crate) unsafe extern "C" fn semphr_create(max: u32, init: u32) -> *mut c_void {
    unsafe { legacy_osi::semphr_create(max, init) }
}

pub(crate) unsafe extern "C" fn semphr_delete(semphr: *mut c_void) {
    unsafe { legacy_osi::semphr_delete(semphr) }
}

pub(crate) unsafe extern "C" fn semphr_take(semphr: *mut c_void, tick: u32) -> i32 {
    unsafe { legacy_osi::semphr_take(semphr, tick) }
}

pub(crate) unsafe extern "C" fn semphr_give(semphr: *mut c_void) -> i32 {
    unsafe { legacy_osi::semphr_give(semphr) }
}

pub(crate) unsafe extern "C" fn semphr_take_from_isr(
    semphr: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    legacy_osi::semphr_take_from_isr(semphr, higher_priority_task_waken.cast())
}

pub(crate) unsafe extern "C" fn semphr_give_from_isr(
    semphr: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    legacy_osi::semphr_give_from_isr(semphr, higher_priority_task_waken.cast())
}

pub(crate) unsafe extern "C" fn wifi_thread_semphr_get() -> *mut c_void {
    unsafe { legacy_osi::wifi_thread_semphr_get() }
}

pub(crate) unsafe extern "C" fn mutex_create() -> *mut c_void {
    unsafe { legacy_osi::mutex_create() }
}

pub(crate) unsafe extern "C" fn recursive_mutex_create() -> *mut c_void {
    unsafe { legacy_osi::recursive_mutex_create() }
}

pub(crate) unsafe extern "C" fn mutex_delete(mutex: *mut c_void) {
    unsafe { legacy_osi::mutex_delete(mutex) }
}

pub(crate) unsafe extern "C" fn mutex_lock(mutex: *mut c_void) -> i32 {
    unsafe { legacy_osi::mutex_lock(mutex) }
}

pub(crate) unsafe extern "C" fn mutex_unlock(mutex: *mut c_void) -> i32 {
    unsafe { legacy_osi::mutex_unlock(mutex) }
}

pub(crate) unsafe extern "C" fn queue_create(
    queue_len: u32,
    item_size: u32,
) -> *mut crate::binary::c_types::c_void {
    unsafe { legacy_osi::queue_create(queue_len, item_size) }
}

pub(crate) unsafe extern "C" fn queue_delete(queue: *mut crate::binary::c_types::c_void) {
    unsafe { legacy_osi::queue_delete(queue) }
}

pub(crate) unsafe extern "C" fn queue_send(
    queue: *mut crate::binary::c_types::c_void,
    item: *mut crate::binary::c_types::c_void,
    block_time_tick: u32,
) -> i32 {
    unsafe { legacy_osi::queue_send(queue, item, block_time_tick) }
}

pub(crate) unsafe extern "C" fn queue_send_from_isr(
    queue: *mut crate::binary::c_types::c_void,
    item: *mut crate::binary::c_types::c_void,
    higher_priority_task_waken: *mut crate::binary::c_types::c_void,
) -> i32 {
    unsafe { legacy_osi::queue_send_from_isr(queue, item, higher_priority_task_waken.cast()) }
}

pub(crate) unsafe extern "C" fn queue_send_to_back(
    queue: *mut crate::binary::c_types::c_void,
    item: *mut crate::binary::c_types::c_void,
    block_time_tick: u32,
) -> i32 {
    unsafe { legacy_osi::queue_send_to_back(queue, item, block_time_tick) }
}

pub(crate) unsafe extern "C" fn queue_send_to_front(
    queue: *mut crate::binary::c_types::c_void,
    item: *mut crate::binary::c_types::c_void,
    block_time_tick: u32,
) -> i32 {
    unsafe { legacy_osi::queue_send_to_front(queue, item, block_time_tick) }
}

pub(crate) unsafe extern "C" fn queue_recv(
    queue: *mut crate::binary::c_types::c_void,
    item: *mut crate::binary::c_types::c_void,
    block_time_tick: u32,
) -> i32 {
    unsafe { legacy_osi::queue_recv(queue, item, block_time_tick) }
}

pub(crate) unsafe extern "C" fn queue_msg_waiting(
    queue: *mut crate::binary::c_types::c_void,
) -> u32 {
    unsafe { legacy_osi::queue_msg_waiting(queue) }
}

pub(crate) unsafe extern "C" fn event_group_create() -> *mut c_void {
    unsafe { legacy_osi::event_group_create() }
}

pub(crate) unsafe extern "C" fn event_group_delete(event: *mut c_void) {
    unsafe { legacy_osi::event_group_delete(event) }
}

pub(crate) unsafe extern "C" fn event_group_set_bits(event: *mut c_void, bits: u32) -> u32 {
    unsafe { legacy_osi::event_group_set_bits(event, bits) }
}

pub(crate) unsafe extern "C" fn event_group_clear_bits(event: *mut c_void, bits: u32) -> u32 {
    unsafe { legacy_osi::event_group_clear_bits(event, bits) }
}

pub(crate) unsafe extern "C" fn event_group_wait_bits(
    event: *mut c_void,
    bits_to_wait_for: u32,
    clear_on_exit: i32,
    wait_for_all_bits: i32,
    block_time_tick: u32,
) -> u32 {
    unsafe { legacy_osi::event_group_wait_bits(
        event,
        bits_to_wait_for,
        clear_on_exit,
        wait_for_all_bits,
        block_time_tick,
    ) }
}

pub(crate) unsafe extern "C" fn task_create_result(
    task_func: *mut c_void,
    name: *const c_char,
    stack_depth: u32,
    param: *mut c_void,
    prio: u32,
    task_handle: *mut c_void,
) -> i32 {
    unsafe { legacy_osi::task_create(task_func, name, stack_depth, param, prio, task_handle) }
}

pub(crate) unsafe extern "C" fn task_create(
    task_func: *mut c_void,
    name: *const c_char,
    stack_depth: u32,
    param: *mut c_void,
    prio: u32,
    task_handle: *mut c_void,
) -> i32 {
    unsafe { task_create_result(task_func, name, stack_depth, param, prio, task_handle) }
}

pub(crate) unsafe extern "C" fn task_create_pinned_to_core_result(
    task_func: *mut c_void,
    name: *const c_char,
    stack_depth: u32,
    param: *mut c_void,
    prio: u32,
    task_handle: *mut c_void,
    core_id: u32,
) -> i32 {
    unsafe {
        legacy_osi::task_create_pinned_to_core(
            task_func,
            name,
            stack_depth,
            param,
            prio,
            task_handle,
            core_id,
        )
    }
}

pub(crate) unsafe extern "C" fn task_create_pinned_to_core(
    task_func: *mut c_void,
    name: *const c_char,
    stack_depth: u32,
    param: *mut c_void,
    prio: u32,
    task_handle: *mut c_void,
    core_id: u32,
) -> i32 {
    unsafe {
        task_create_pinned_to_core_result(
            task_func,
            name,
            stack_depth,
            param,
            prio,
            task_handle,
            core_id,
        )
    }
}

pub(crate) unsafe extern "C" fn task_delete(task_handle: *mut c_void) {
    unsafe { legacy_osi::task_delete(task_handle) }
}

pub(crate) unsafe extern "C" fn task_delay(tick: u32) {
    unsafe { legacy_osi::task_delay(tick) }
}

pub(crate) unsafe extern "C" fn task_ms_to_tick(ms: u32) -> i32 {
    unsafe { legacy_osi::task_ms_to_tick(ms) }
}

pub(crate) unsafe extern "C" fn task_get_current_task() -> *mut c_void {
    unsafe { legacy_osi::task_get_current_task() }
}

pub(crate) unsafe extern "C" fn task_get_max_priority() -> i32 {
    unsafe { legacy_osi::task_get_max_priority() }
}

pub(crate) unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    unsafe { legacy_osi::malloc(size) }
}

pub(crate) unsafe extern "C" fn free(ptr: *mut c_void) {
    unsafe { legacy_osi::free(ptr) }
}

pub(crate) unsafe extern "C" fn esp_timer_get_time() -> i64 {
    unsafe { legacy_osi::esp_timer_get_time() }
}

pub(crate) unsafe extern "C" fn event_post(
    event_base: *const c_char,
    event_id: i32,
    event_data: *mut c_void,
    event_data_size: usize,
    ticks_to_wait: u32,
) -> i32 {
    unsafe { legacy_osi::event_post(event_base, event_id, event_data, event_data_size, ticks_to_wait) }
}

pub(crate) unsafe extern "C" fn get_free_heap_size() -> u32 {
    unsafe { legacy_osi::get_free_heap_size() }
}

pub(crate) unsafe extern "C" fn rand() -> u32 {
    unsafe { legacy_osi::rand() }
}

pub(crate) unsafe extern "C" fn get_random(buf: *mut u8, len: usize) -> i32 {
    unsafe { legacy_osi::get_random(buf, len) }
}

pub(crate) unsafe extern "C" fn get_time(t: *mut c_void) -> i32 {
    unsafe { legacy_osi::get_time(t) }
}

pub(crate) unsafe extern "C" fn random() -> u32 {
    unsafe { legacy_osi::random() }
}

pub(crate) unsafe extern "C" fn log_timestamp() -> u32 {
    unsafe { legacy_osi::log_timestamp() }
}

pub(crate) unsafe extern "C" fn malloc_internal(size: usize) -> *mut c_void {
    unsafe { legacy_osi::malloc_internal(size) }
}

pub(crate) unsafe extern "C" fn realloc_internal(
    ptr: *mut c_void,
    size: usize,
) -> *mut c_void {
    unsafe { legacy_osi::realloc_internal(ptr, size) }
}

pub(crate) unsafe extern "C" fn calloc_internal(n: usize, size: usize) -> *mut c_void {
    unsafe { legacy_osi::calloc_internal(n, size) }
}

pub(crate) unsafe extern "C" fn zalloc_internal(size: usize) -> *mut c_void {
    unsafe { legacy_osi::zalloc_internal(size) }
}

pub(crate) unsafe extern "C" fn wifi_malloc(size: usize) -> *mut c_void {
    unsafe { legacy_osi::wifi_malloc(size) }
}

pub(crate) unsafe extern "C" fn wifi_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    unsafe { legacy_osi::wifi_realloc(ptr, size) }
}

pub(crate) unsafe extern "C" fn wifi_calloc(n: usize, size: usize) -> *mut c_void {
    unsafe { legacy_osi::wifi_calloc(n, size) }
}

pub(crate) unsafe extern "C" fn wifi_zalloc(size: usize) -> *mut c_void {
    unsafe { legacy_osi::wifi_zalloc(size) }
}

pub(crate) unsafe extern "C" fn wifi_create_queue(
    queue_len: i32,
    item_size: i32,
) -> *mut crate::binary::c_types::c_void {
    unsafe { legacy_osi::wifi_create_queue(queue_len, item_size) }
}

pub(crate) unsafe extern "C" fn wifi_delete_queue(queue: *mut crate::binary::c_types::c_void) {
    unsafe { legacy_osi::wifi_delete_queue(queue) }
}

pub(crate) unsafe extern "C" fn coex_status_get() -> u32 {
    unsafe { legacy_osi::coex_status_get() as u32 }
}
