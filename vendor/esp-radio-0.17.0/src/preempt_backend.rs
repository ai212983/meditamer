use core::{ffi::c_void, ptr::NonNull};

pub use esp_radio_rtos_driver::{queue, semaphore, timer};

fn backend_legacy_port_enabled() -> bool {
    crate::compat::legacy_runtime_policy::backend_legacy_port_enabled()
}

unsafe extern "C" {
    fn __esp_rtos_legacy_preempt_builtin_enable();
    fn __esp_rtos_legacy_preempt_builtin_yield_task();
    fn __esp_rtos_legacy_preempt_builtin_current_task() -> *mut c_void;
    fn __esp_rtos_legacy_preempt_builtin_current_task_thread_semaphore() -> *mut c_void;
    fn __esp_rtos_legacy_preempt_builtin_task_create(
        task: extern "C" fn(*mut c_void),
        param: *mut c_void,
        task_stack_size: usize,
    ) -> *mut c_void;
    fn __esp_rtos_legacy_preempt_builtin_schedule_task_deletion(task: *mut c_void);
    fn __esp_rtos_legacy_preempt_builtin_max_task_priority() -> u32;
}

pub fn enable() {
    if backend_legacy_port_enabled() {
        unsafe { __esp_rtos_legacy_preempt_builtin_enable() };
    }
}

pub fn initialized() -> bool {
    if backend_legacy_port_enabled() {
        return !unsafe { __esp_rtos_legacy_preempt_builtin_current_task() }.is_null();
    }
    esp_radio_rtos_driver::initialized()
}

pub fn yield_task() {
    if backend_legacy_port_enabled() {
        unsafe { __esp_rtos_legacy_preempt_builtin_yield_task() };
        return;
    }
    esp_radio_rtos_driver::yield_task();
}

pub fn yield_task_from_isr() {
    if backend_legacy_port_enabled() {
        unsafe { __esp_rtos_legacy_preempt_builtin_yield_task() };
        return;
    }
    esp_radio_rtos_driver::yield_task_from_isr();
}

pub fn current_task() -> *mut c_void {
    if backend_legacy_port_enabled() {
        return unsafe { __esp_rtos_legacy_preempt_builtin_current_task() };
    }
    esp_radio_rtos_driver::current_task()
}

pub fn current_task_thread_semaphore() -> NonNull<c_void> {
    if backend_legacy_port_enabled() {
        return NonNull::new(unsafe { __esp_rtos_legacy_preempt_builtin_current_task_thread_semaphore() })
            .expect("legacy current_task_thread_semaphore returned null");
    }
    esp_radio_rtos_driver::current_task_thread_semaphore().cast()
}

pub fn task_create(
    name: &str,
    task: extern "C" fn(*mut c_void),
    param: *mut c_void,
    priority: u32,
    pin_to_core: Option<u32>,
    task_stack_size: usize,
) -> *mut c_void {
    let _ = name;
    let _ = priority;
    let _ = pin_to_core;
    if backend_legacy_port_enabled() {
        return unsafe { __esp_rtos_legacy_preempt_builtin_task_create(task, param, task_stack_size) };
    }
    unsafe {
        esp_radio_rtos_driver::task_create(name, task, param, priority, pin_to_core, task_stack_size)
    }
}

pub fn schedule_task_deletion(task: *mut c_void) {
    if backend_legacy_port_enabled() {
        unsafe { __esp_rtos_legacy_preempt_builtin_schedule_task_deletion(task) };
        return;
    }
    unsafe { esp_radio_rtos_driver::schedule_task_deletion(task) };
}

pub fn max_task_priority() -> u32 {
    if backend_legacy_port_enabled() {
        return unsafe { __esp_rtos_legacy_preempt_builtin_max_task_priority() };
    }
    esp_radio_rtos_driver::max_task_priority()
}

pub fn usleep(us: u32) {
    esp_radio_rtos_driver::usleep(us);
}

pub fn now() -> u64 {
    esp_radio_rtos_driver::now()
}
