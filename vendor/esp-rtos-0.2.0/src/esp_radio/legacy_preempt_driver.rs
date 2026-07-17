use core::{ffi::c_void, ptr::NonNull};

use esp_hal::time::Instant;
use esp_radio_rtos_driver::semaphore::SemaphorePtr;

use crate::esp_radio::legacy_preempt_builtin;

pub(crate) fn initialized() -> bool {
    legacy_preempt_builtin::initialized()
}

pub(crate) fn yield_task() {
    legacy_preempt_builtin::yield_task();
}

pub(crate) fn yield_task_from_isr() {
    legacy_preempt_builtin::yield_task();
}

pub(crate) fn max_task_priority() -> u32 {
    255
}

pub(crate) fn task_create(
    task: extern "C" fn(*mut c_void),
    param: *mut c_void,
    task_stack_size: usize,
) -> *mut c_void {
    legacy_preempt_builtin::enable();
    legacy_preempt_builtin::task_create(task, param, task_stack_size)
}

pub(crate) fn current_task() -> *mut c_void {
    legacy_preempt_builtin::enable();
    legacy_preempt_builtin::current_task()
}

pub(crate) fn schedule_task_deletion(task_handle: *mut c_void) {
    legacy_preempt_builtin::schedule_task_deletion(task_handle)
}

pub(crate) fn current_task_thread_semaphore() -> SemaphorePtr {
    legacy_preempt_builtin::enable();
    NonNull::new(legacy_preempt_builtin::current_task_thread_semaphore())
        .unwrap()
        .cast()
}

pub(crate) fn usleep(us: u32) {
    unsafe extern "C" {
        fn esp_rom_delay_us(us: u32);
    }

    unsafe {
        esp_rom_delay_us(us);
    }
}

pub(crate) fn now_us() -> u64 {
    Instant::now().duration_since_epoch().as_micros()
}
