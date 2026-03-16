use core::ffi::{c_char, c_void};
use num_traits::FromPrimitive;
use portable_atomic::{AtomicU32, Ordering};

use crate::{
    compat::{
        common_legacy_literal,
        common_legacy_port,
        malloc,
    },
    memory_fence::memory_fence,
    time,
    wifi::{self, internal_legacy_event_literal, os_adapter::WIFI_EVENTS, WifiEvent},
};

static mut WIFI_STATIC_QUEUE_HANDLE: *mut c_void = core::ptr::null_mut();

#[derive(Clone, Copy)]
pub(crate) struct InternalLegacyEventPostDiag {
    pub count: u32,
    pub last_event_id: i32,
    pub scan_done_status: u32,
    pub scan_done_number: u32,
    pub scan_done_id: u32,
    pub scan_done_ap_num_rc: u32,
    pub scan_done_ap_num: u32,
}

static INTERNAL_LEGACY_EVENT_POST_COUNT: AtomicU32 = AtomicU32::new(0);
static INTERNAL_LEGACY_EVENT_POST_LAST_EVENT_ID: AtomicU32 = AtomicU32::new(0);
static INTERNAL_LEGACY_SCAN_DONE_STATUS: AtomicU32 = AtomicU32::new(0);
static INTERNAL_LEGACY_SCAN_DONE_NUMBER: AtomicU32 = AtomicU32::new(0);
static INTERNAL_LEGACY_SCAN_DONE_ID: AtomicU32 = AtomicU32::new(0);
static INTERNAL_LEGACY_SCAN_DONE_AP_NUM_RC: AtomicU32 = AtomicU32::new(0);
static INTERNAL_LEGACY_SCAN_DONE_AP_NUM: AtomicU32 = AtomicU32::new(0);

fn legacy_random_u32() -> u32 {
    let mut rng = esp_hal::rng::Rng::new();
    rng.random()
}

unsafe fn legacy_fill_random(dst: *mut u8, len: usize) {
    let dst = unsafe { core::slice::from_raw_parts_mut(dst, len) };
    let mut rng = esp_hal::rng::Rng::new();
    for chunk in dst.chunks_mut(4) {
        let bytes = rng.random().to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
}

pub(crate) unsafe extern "C" fn task_yield_from_isr() {
    common_legacy_literal::task_yield_from_isr();
}

pub(crate) unsafe extern "C" fn semphr_create(_max: u32, init: u32) -> *mut c_void {
    common_legacy_literal::sem_create(_max, init)
}

pub(crate) unsafe extern "C" fn semphr_delete(semphr: *mut c_void) {
    common_legacy_literal::sem_delete(semphr);
}

pub(crate) unsafe extern "C" fn semphr_take(semphr: *mut c_void, tick: u32) -> i32 {
    common_legacy_literal::sem_take(semphr, tick)
}

pub(crate) unsafe extern "C" fn semphr_give(semphr: *mut c_void) -> i32 {
    common_legacy_literal::sem_give(semphr)
}

pub(crate) unsafe extern "C" fn semphr_take_from_isr(
    semphr: *mut c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    common_legacy_literal::sem_take_from_isr(semphr, higher_priority_task_waken)
}

pub(crate) unsafe extern "C" fn semphr_take_from_isr_c_void(
    semphr: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    unsafe { semphr_take_from_isr(semphr, higher_priority_task_waken.cast()) }
}

pub(crate) unsafe extern "C" fn semphr_give_from_isr(
    semphr: *mut c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    common_legacy_literal::sem_give_from_isr(semphr, higher_priority_task_waken)
}

pub(crate) unsafe extern "C" fn semphr_give_from_isr_c_void(
    semphr: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    unsafe { semphr_give_from_isr(semphr, higher_priority_task_waken.cast()) }
}

pub(crate) unsafe extern "C" fn wifi_thread_semphr_get() -> *mut c_void {
    common_legacy_literal::thread_sem_get()
}

pub(crate) unsafe extern "C" fn mutex_create() -> *mut c_void {
    common_legacy_literal::create_mutex()
}

pub(crate) unsafe extern "C" fn recursive_mutex_create() -> *mut c_void {
    common_legacy_literal::create_recursive_mutex()
}

pub(crate) unsafe extern "C" fn mutex_delete(mutex: *mut c_void) {
    common_legacy_literal::mutex_delete(mutex);
}

pub(crate) unsafe extern "C" fn mutex_lock(mutex: *mut c_void) -> i32 {
    common_legacy_literal::lock_mutex(mutex)
}

pub(crate) unsafe extern "C" fn mutex_unlock(mutex: *mut c_void) -> i32 {
    common_legacy_literal::unlock_mutex(mutex)
}

pub(crate) unsafe extern "C" fn queue_create(queue_len: u32, item_size: u32) -> *mut c_void {
    common_legacy_literal::create_queue(queue_len as i32, item_size as i32).cast()
}

pub(crate) unsafe extern "C" fn queue_delete(queue: *mut c_void) {
    common_legacy_literal::delete_queue(queue.cast());
}

pub(crate) unsafe extern "C" fn queue_send(
    queue: *mut c_void,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    common_legacy_literal::send_queued(queue.cast(), item, block_time_tick)
}

pub(crate) unsafe extern "C" fn queue_send_from_isr(
    queue: *mut c_void,
    item: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    common_legacy_literal::try_send_queued_from_isr(queue.cast(), item, higher_priority_task_waken.cast())
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

pub(crate) unsafe extern "C" fn queue_msg_waiting(queue: *mut c_void) -> u32 {
    common_legacy_literal::number_of_messages_in_queue(queue.cast())
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

pub(crate) unsafe extern "C" fn task_create(
    task_func: *mut c_void,
    _name: *const c_char,
    stack_depth: u32,
    param: *mut c_void,
    _prio: u32,
    task_handle: *mut c_void,
) -> i32 {
    {
        let _ = _prio;
        common_legacy_literal::task_create(task_func, _name, stack_depth, param, task_handle)
    }
}

pub(crate) unsafe extern "C" fn task_create_pinned_to_core(
    task_func: *mut c_void,
    name: *const c_char,
    stack_depth: u32,
    param: *mut c_void,
    prio: u32,
    task_handle: *mut c_void,
    _core_id: u32,
) -> i32 {
    {
        let _ = (prio, _core_id);
        common_legacy_literal::task_create(task_func, name, stack_depth, param, task_handle)
    }
}

pub(crate) unsafe extern "C" fn task_delete(task_handle: *mut c_void) {
    common_legacy_literal::task_delete(task_handle);
}

pub(crate) unsafe extern "C" fn task_delay(tick: u32) {
    common_legacy_literal::task_delay(tick);
}

pub(crate) unsafe extern "C" fn task_ms_to_tick(ms: u32) -> i32 {
    time::millis_to_blob_ticks(ms) as i32
}

pub(crate) unsafe extern "C" fn task_get_current_task() -> *mut c_void {
    common_legacy_literal::task_get_current_task()
}

pub(crate) unsafe extern "C" fn task_get_max_priority() -> i32 {
    common_legacy_literal::task_get_max_priority()
}

pub(crate) unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    malloc::malloc(size).cast()
}

pub(crate) unsafe extern "C" fn free(ptr: *mut c_void) {
    malloc::free(ptr.cast());
}

pub(crate) unsafe extern "C" fn esp_timer_get_time() -> i64 {
    esp_hal::time::Instant::now().duration_since_epoch().as_micros() as i64
}

pub(crate) unsafe extern "C" fn event_post(
    _event_base: *const c_char,
    event_id: i32,
    event_data: *mut c_void,
    event_data_size: usize,
    _ticks_to_wait: u32,
) -> i32 {
    INTERNAL_LEGACY_EVENT_POST_COUNT.fetch_add(1, Ordering::Relaxed);
    INTERNAL_LEGACY_EVENT_POST_LAST_EVENT_ID.store(event_id as u32, Ordering::Relaxed);
    let event = WifiEvent::from_i32(event_id).expect("invalid wifi event id");
    if matches!(event, WifiEvent::ScanDone) {
        let mut ap_num: u16 = 0;
        let ap_num_rc = unsafe { crate::binary::include::esp_wifi_scan_get_ap_num(&mut ap_num) };
        let scan = unsafe {
            &*(event_data.cast::<crate::binary::include::wifi_event_sta_scan_done_t>())
        };
        INTERNAL_LEGACY_SCAN_DONE_STATUS.store(scan.status, Ordering::Relaxed);
        INTERNAL_LEGACY_SCAN_DONE_NUMBER.store(u32::from(scan.number), Ordering::Relaxed);
        INTERNAL_LEGACY_SCAN_DONE_ID.store(u32::from(scan.scan_id), Ordering::Relaxed);
        INTERNAL_LEGACY_SCAN_DONE_AP_NUM_RC.store(ap_num_rc as u32, Ordering::Relaxed);
        INTERNAL_LEGACY_SCAN_DONE_AP_NUM.store(u32::from(ap_num), Ordering::Relaxed);
    }
    WIFI_EVENTS.with(|events| events.insert(event));

    let handled = internal_legacy_event_literal::dispatch_event_handler(
        event,
        event_data,
        event_data_size,
    );
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

pub(crate) fn internal_legacy_event_post_diag() -> InternalLegacyEventPostDiag {
    InternalLegacyEventPostDiag {
        count: INTERNAL_LEGACY_EVENT_POST_COUNT.load(Ordering::Relaxed),
        last_event_id: INTERNAL_LEGACY_EVENT_POST_LAST_EVENT_ID.load(Ordering::Relaxed) as i32,
        scan_done_status: INTERNAL_LEGACY_SCAN_DONE_STATUS.load(Ordering::Relaxed),
        scan_done_number: INTERNAL_LEGACY_SCAN_DONE_NUMBER.load(Ordering::Relaxed),
        scan_done_id: INTERNAL_LEGACY_SCAN_DONE_ID.load(Ordering::Relaxed),
        scan_done_ap_num_rc: INTERNAL_LEGACY_SCAN_DONE_AP_NUM_RC.load(Ordering::Relaxed),
        scan_done_ap_num: INTERNAL_LEGACY_SCAN_DONE_AP_NUM.load(Ordering::Relaxed),
    }
}

pub(crate) unsafe extern "C" fn get_free_heap_size() -> u32 {
    malloc::get_free_internal_heap_size() as u32
}

pub(crate) unsafe extern "C" fn rand() -> u32 {
    legacy_random_u32()
}

pub(crate) unsafe extern "C" fn get_random(buf: *mut u8, len: usize) -> i32 {
    unsafe { legacy_fill_random(buf, len) };
    0
}

pub(crate) unsafe extern "C" fn get_time(_t: *mut c_void) -> i32 {
    todo!("get_time")
}

pub(crate) unsafe extern "C" fn random() -> u32 {
    legacy_random_u32()
}

pub(crate) unsafe extern "C" fn log_timestamp() -> u32 {
    esp_hal::time::Instant::now().duration_since_epoch().as_millis() as u32
}

pub(crate) unsafe extern "C" fn malloc_internal(size: usize) -> *mut c_void {
    malloc(size)
}

pub(crate) unsafe extern "C" fn realloc_internal(ptr: *mut c_void, size: usize) -> *mut c_void {
    malloc::realloc_internal(ptr.cast(), size).cast()
}

pub(crate) unsafe extern "C" fn calloc_internal(n: usize, size: usize) -> *mut c_void {
    malloc::calloc(n as u32, size).cast()
}

pub(crate) unsafe extern "C" fn zalloc_internal(size: usize) -> *mut c_void {
    malloc::calloc(size as u32, 1).cast()
}

pub(crate) unsafe extern "C" fn wifi_malloc(size: usize) -> *mut c_void {
    malloc(size)
}

pub(crate) unsafe extern "C" fn wifi_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    realloc_internal(ptr, size)
}

pub(crate) unsafe extern "C" fn wifi_calloc(n: usize, size: usize) -> *mut c_void {
    malloc::calloc(n as u32, size).cast()
}

pub(crate) unsafe extern "C" fn wifi_zalloc(size: usize) -> *mut c_void {
    wifi_calloc(size, 1)
}

pub(crate) unsafe extern "C" fn wifi_create_queue(queue_len: i32, item_size: i32) -> *mut c_void {
    let queue = common_legacy_literal::create_queue(queue_len, item_size).cast();
    WIFI_STATIC_QUEUE_HANDLE = queue;
    core::ptr::addr_of_mut!(WIFI_STATIC_QUEUE_HANDLE).cast()
}

pub(crate) unsafe extern "C" fn wifi_delete_queue(queue: *mut c_void) {
    if core::ptr::eq(queue, core::ptr::addr_of_mut!(WIFI_STATIC_QUEUE_HANDLE).cast()) {
        common_legacy_literal::delete_queue(WIFI_STATIC_QUEUE_HANDLE.cast());
        WIFI_STATIC_QUEUE_HANDLE = core::ptr::null_mut();
    }
}

pub(crate) unsafe extern "C" fn coex_status_get() -> u32 {
    #[cfg(coex)]
    {
        return crate::binary::include::coex_status_get();
    }

    #[cfg(not(coex))]
    0
}
