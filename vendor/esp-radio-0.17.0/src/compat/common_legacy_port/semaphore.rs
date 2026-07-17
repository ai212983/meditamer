use core::ffi::c_void;

use crate::{
    compat::{
        malloc::{free, malloc},
        OSI_FUNCS_TIME_BLOCKING,
    },
    memory_fence::memory_fence,
    time,
};

pub(crate) unsafe fn semphr_create(max: u32, init: u32) -> *mut c_void {
    let _ = max;
    let ptr = unsafe { malloc(4) as *mut u32 };
    unsafe { ptr.write_volatile(init) };
    ptr.cast()
}

pub(crate) unsafe fn semphr_delete(semphr: *mut c_void) {
    unsafe { free(semphr.cast()) };
}

pub(crate) unsafe fn semphr_take(semphr: *mut c_void, tick: u32) -> i32 {
    // Legacy preempt_builtin treated a blocking take from ISR as a last-resort
    // early success instead of spinning forever.
    let tick = if tick == OSI_FUNCS_TIME_BLOCKING && crate::is_interrupts_disabled() {
        1
    } else {
        tick
    };

    let forever = tick == OSI_FUNCS_TIME_BLOCKING;
    let timeout = tick as u64;
    let start = time::systimer_count();
    let sem = semphr.cast::<u32>();

    'outer: loop {
        let res = critical_section::with(|_| unsafe {
            memory_fence();
            let cnt = *sem;
            if cnt > 0 {
                *sem = cnt - 1;
                1
            } else {
                0
            }
        });

        if res == 1 {
            return 1;
        }

        if !forever && time::elapsed_time_since(start) > timeout {
            break 'outer;
        }

        crate::compat::preempt_legacy_backend::yield_task();
    }

    0
}

pub(crate) unsafe fn semphr_take_from_isr(
    semphr: *mut c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    let sem = semphr.cast::<u32>();
    critical_section::with(|_| unsafe {
        let cnt = *sem;
        if cnt > 0 {
            *sem = cnt - 1;
            if let Some(waken) = higher_priority_task_waken.as_mut() {
                *waken = true;
            }
            1
        } else {
            0
        }
    })
}

pub(crate) unsafe fn semphr_give(semphr: *mut c_void) -> i32 {
    let sem = semphr.cast::<u32>();
    critical_section::with(|_| unsafe {
        let cnt = *sem;
        *sem = cnt + 1;
        1
    })
}

pub(crate) unsafe fn semphr_give_from_isr(
    semphr: *mut c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    let sem = semphr.cast::<u32>();
    critical_section::with(|_| unsafe {
        let cnt = *sem;
        *sem = cnt + 1;
        if let Some(waken) = higher_priority_task_waken.as_mut() {
            *waken = true;
        }
        1
    })
}
