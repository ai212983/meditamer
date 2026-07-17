use core::{ffi::c_void, mem::size_of};

use crate::compat::malloc::{free, malloc};

#[repr(C)]
struct LegacyMutex {
    locking_pid: usize,
    count: u32,
    recursive: bool,
}

fn legacy_mutex_ptr(mutex: *mut c_void) -> *mut LegacyMutex {
    mutex.cast()
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

pub(crate) unsafe fn mutex_delete(mutex: *mut c_void) {
    unsafe { free(mutex.cast()) };
}

pub(crate) unsafe fn mutex_lock(mutex: *mut c_void) -> i32 {
    let ptr = legacy_mutex_ptr(mutex);
    let current_task = crate::compat::preempt_legacy_backend::current_task() as usize;

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
        crate::compat::preempt_legacy_backend::yield_task();
    }
}

pub(crate) unsafe fn mutex_unlock(mutex: *mut c_void) -> i32 {
    let ptr = legacy_mutex_ptr(mutex);
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
