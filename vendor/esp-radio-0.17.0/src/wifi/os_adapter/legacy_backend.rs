use crate::{
    binary::c_types::c_void,
    compat::common_legacy_literal,
};
use esp_wifi_sys::c_types::c_char;

pub(crate) unsafe extern "C" fn task_yield_from_isr() {
    common_legacy_literal::task_yield_from_isr();
}

pub(crate) unsafe extern "C" fn wifi_thread_semphr_get() -> *mut c_void {
    common_legacy_literal::thread_sem_get()
}

unsafe fn mutex_create_impl(recursive: bool) -> *mut c_void {
    if recursive {
        common_legacy_literal::create_recursive_mutex()
    } else {
        common_legacy_literal::create_mutex()
    }
}

pub(crate) unsafe extern "C" fn mutex_create() -> *mut c_void {
    unsafe { mutex_create_impl(false) }
}

pub(crate) unsafe extern "C" fn recursive_mutex_create() -> *mut c_void {
    unsafe { mutex_create_impl(true) }
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

pub(crate) unsafe extern "C" fn task_create(
    task_func: *mut c_void,
    name: *const c_char,
    stack_depth: u32,
    param: *mut c_void,
    prio: u32,
    task_handle: *mut c_void,
    core_id: Option<u32>,
) -> *mut c_void {
    unsafe {
        let _ = core_id;
        let _ = prio;
        common_legacy_literal::task_create(task_func, name, stack_depth, param, task_handle);
        *(task_handle as *mut usize) as *mut c_void
    }
}

pub(crate) unsafe extern "C" fn task_create_result(
    task_func: *mut c_void,
    name: *const c_char,
    stack_depth: u32,
    param: *mut c_void,
    prio: u32,
    task_handle: *mut c_void,
) -> i32 {
    let _ = unsafe { task_create(task_func, name, stack_depth, param, prio, task_handle, None) };
    1
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
    let _ = unsafe {
        task_create(
            task_func,
            name,
            stack_depth,
            param,
            prio,
            task_handle,
            Some(core_id),
        )
    };
    1
}

pub(crate) unsafe extern "C" fn task_delete(task_handle: *mut c_void) {
    common_legacy_literal::task_delete(task_handle);
}

pub(crate) unsafe extern "C" fn task_delay(tick: u32) {
    common_legacy_literal::task_delay(tick);
}

pub(crate) unsafe extern "C" fn task_ms_to_tick(ms: u32) -> i32 {
    ms as i32
}

pub(crate) unsafe extern "C" fn task_get_current_task() -> *mut c_void {
    common_legacy_literal::task_get_current_task()
}

pub(crate) unsafe extern "C" fn task_get_max_priority() -> i32 {
    common_legacy_literal::task_get_max_priority()
}

#[cfg(coex)]
pub(crate) unsafe extern "C" fn coex_status_get() -> u32 {
    crate::binary::include::coex_status_get(0b1)
}

#[cfg(not(coex))]
pub(crate) unsafe extern "C" fn coex_status_get() -> u32 {
    0
}
