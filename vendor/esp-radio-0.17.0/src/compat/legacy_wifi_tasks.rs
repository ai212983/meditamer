use core::{ffi::c_void, ptr::null_mut};

use portable_atomic::{AtomicBool, Ordering};

use crate::{compat::timer_compat_legacy, preempt_backend};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LegacyWifiTasksInitStatus {
    pub timer_task_precreated: bool,
    pub yielded_once: bool,
}

static TIMER_TASK_CREATED: AtomicBool = AtomicBool::new(false);

extern "C" fn timer_task(_param: *mut c_void) {
    loop {
        if !timer_compat_legacy::process_due_timer() {
            preempt_backend::yield_task();
        }
    }
}

pub fn init_legacy_wifi_tasks() -> LegacyWifiTasksInitStatus {
    let mut precreated = false;

    if TIMER_TASK_CREATED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        let _ = preempt_backend::task_create(
            "timer",
            timer_task,
            null_mut(),
            preempt_backend::max_task_priority(),
            None,
            8192,
        );
        precreated = true;
    }

    preempt_backend::yield_task();

    LegacyWifiTasksInitStatus {
        timer_task_precreated: precreated,
        yielded_once: true,
    }
}
