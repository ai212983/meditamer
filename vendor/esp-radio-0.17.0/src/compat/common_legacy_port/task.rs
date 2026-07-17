use core::ffi::c_void;

use esp_wifi_sys::c_types::c_char;

use crate::{
    compat::{common::str_from_c, preempt_legacy_backend as legacy_preempt},
    time,
};

pub(crate) unsafe fn esp_timer_get_time() -> i64 {
    time::systimer_count() as i64
}

pub(crate) unsafe fn task_yield_from_isr() {
    legacy_preempt::yield_task();
}

pub(crate) unsafe fn task_create(
    task_func: *mut c_void,
    name: *const c_char,
    stack_depth: u32,
    param: *mut c_void,
    _prio: u32,
    task_handle: *mut c_void,
    _core_id: Option<u32>,
) -> i32 {
    let _task_name = unsafe { str_from_c(name as _) };
    let task_func =
        unsafe { core::mem::transmute::<*mut c_void, extern "C" fn(*mut c_void)>(task_func) };
    let task = legacy_preempt::task_create(task_func, param, stack_depth as usize);
    unsafe { *(task_handle as *mut usize) = task as usize };
    1
}

pub(crate) unsafe fn task_delete(task_handle: *mut c_void) {
    let task = if task_handle.is_null() {
        legacy_preempt::current_task()
    } else {
        task_handle
    };
    legacy_preempt::schedule_task_deletion(task);
}

pub(crate) unsafe fn task_delay(tick: u32) {
    let start_time = time::systimer_count();
    while time::elapsed_time_since(start_time) < tick as u64 {
        legacy_preempt::yield_task();
    }
}

pub(crate) unsafe fn task_current() -> *mut c_void {
    legacy_preempt::current_task() as *mut c_void
}

pub(crate) fn task_ms_to_tick(ms: u32) -> i32 {
    time::millis_to_blob_ticks(ms) as i32
}

pub(crate) unsafe fn task_max_priority() -> i32 {
    legacy_preempt::max_task_priority() as i32
}
