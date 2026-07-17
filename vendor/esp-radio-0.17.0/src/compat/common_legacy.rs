#![allow(unused)]

use core::{ffi::c_void, ptr::NonNull};

use esp_wifi_sys::c_types::c_char;

use crate::{
    compat::{
        common::str_from_c,
        malloc::{free, malloc},
        preempt_legacy_backend as legacy_preempt,
        queue_legacy,
        OSI_FUNCS_TIME_BLOCKING,
    },
    memory_fence::memory_fence,
    time,
};

#[repr(C)]
struct LegacySemaphore {
    count: u32,
}

#[repr(C)]
struct LegacyMutex {
    locking_pid: usize,
    count: u32,
    recursive: bool,
}

fn legacy_semaphore_ptr(semphr: *mut c_void) -> *mut LegacySemaphore {
    semphr.cast()
}

fn legacy_mutex_ptr(mutex: *mut c_void) -> *mut LegacyMutex {
    mutex.cast()
}

pub(crate) fn thread_sem_get() -> *mut c_void {
    legacy_preempt::current_task_thread_semaphore()
        .as_ptr()
        .cast::<c_void>()
}

pub(crate) unsafe fn semphr_create(max: u32, init: u32) -> *mut c_void {
    let _ = max;
    let ptr = unsafe { malloc(4) as *mut u32 };
    unsafe {
        ptr.write_volatile(init);
    }
    ptr.cast()
}

pub(crate) unsafe fn semphr_delete(semphr: *mut c_void) {
    unsafe { free(semphr.cast()) };
}

pub(crate) unsafe fn semphr_take(semphr: *mut c_void, tick: u32) -> i32 {
    let tick = if tick == OSI_FUNCS_TIME_BLOCKING && crate::is_interrupts_disabled() {
        1
    } else {
        tick
    };
    let forever = tick == OSI_FUNCS_TIME_BLOCKING;
    let timeout = tick as u64;
    let start = time::systimer_count();
    let sem = semphr.cast::<u32>();

    loop {
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
            return 0;
        }
        legacy_preempt::yield_task();
    }
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

pub(crate) unsafe fn queue_send_from_isr(
    queue_ptr: *mut c_void,
    item: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    if !higher_priority_task_waken.is_null() {
        unsafe { *(higher_priority_task_waken as *mut u32) = 1 };
    }
    queue_legacy::queue_try_send_to_back_from_isr(queue_ptr, item, higher_priority_task_waken.cast())
}

pub(crate) unsafe fn queue_send_to_back(
    queue_ptr: *mut c_void,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    queue_legacy::queue_send_to_back(queue_ptr, item, block_time_tick)
}

pub(crate) unsafe fn queue_send_to_front(
    queue_ptr: *mut c_void,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    queue_legacy::queue_send_to_front(queue_ptr, item, block_time_tick)
}

pub(crate) unsafe fn queue_recv(
    queue_ptr: *mut c_void,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    queue_legacy::queue_receive(queue_ptr, item, block_time_tick)
}

pub(crate) unsafe fn queue_recv_from_isr(
    queue_ptr: *mut c_void,
    item: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    queue_legacy::queue_try_receive_from_isr(queue_ptr, item, higher_priority_task_waken.cast())
}

pub(crate) unsafe fn queue_msg_waiting(queue_ptr: *mut c_void) -> u32 {
    queue_legacy::queue_messages_waiting(queue_ptr)
}

pub(crate) unsafe fn mutex_create(recursive: bool) -> *mut c_void {
    let ptr = unsafe { malloc(size_of::<LegacyMutex>()) as *mut LegacyMutex };
    unsafe {
        ptr.write(LegacyMutex {
            locking_pid: usize::MAX,
            count: 0,
            recursive,
        });
    }
    ptr.cast()
}

pub(crate) unsafe fn mutex_delete(mutex_ptr: *mut c_void) {
    unsafe { free(mutex_ptr.cast()) };
}

pub(crate) unsafe fn mutex_lock(mutex_ptr: *mut c_void) -> i32 {
    let ptr = legacy_mutex_ptr(mutex_ptr);
    let current_task = legacy_preempt::current_task() as usize;

    loop {
        let locked = critical_section::with(|_| unsafe {
            if (*ptr).count == 0 {
                (*ptr).locking_pid = current_task;
                (*ptr).count = 1;
                true
            } else if (*ptr).recursive && (*ptr).locking_pid == current_task {
                (*ptr).count = (*ptr).count.saturating_add(1);
                true
            } else {
                false
            }
        });
        if locked {
            return 1;
        }
        legacy_preempt::yield_task();
    }
}

pub(crate) unsafe fn mutex_unlock(mutex_ptr: *mut c_void) -> i32 {
    let ptr = legacy_mutex_ptr(mutex_ptr);
    critical_section::with(|_| unsafe {
        if (*ptr).count > 0 {
            (*ptr).count -= 1;
            if (*ptr).count == 0 {
                (*ptr).locking_pid = usize::MAX;
            }
            1
        } else {
            0
        }
    })
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
    let task_func = unsafe {
        core::mem::transmute::<*mut c_void, extern "C" fn(*mut esp_wifi_sys::c_types::c_void)>(
            task_func,
        )
    };
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

pub(crate) fn task_ms_to_tick(ms: u32) -> i32 {
    time::millis_to_blob_ticks(ms) as i32
}

pub(crate) fn task_get_current_task() -> *mut c_void {
    legacy_preempt::current_task() as *mut c_void
}

pub(crate) fn task_get_max_priority() -> i32 {
    legacy_preempt::max_task_priority() as i32
}
