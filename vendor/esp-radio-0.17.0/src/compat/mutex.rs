use core::{ffi::c_void, mem::size_of_val};

use esp_radio_rtos_driver::semaphore::SemaphoreKind;

use crate::{
    compat::malloc::{free, malloc},
    memory_fence::memory_fence,
    ESP_RADIO_LOCK,
    preempt::semaphore::{SemaphoreHandle, SemaphorePtr},
};

#[repr(C)]
struct LegacyMutex {
    locking_pid: usize,
    count: u32,
    recursive: bool,
}

fn legacy_mutex_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn legacy_mutex_ptr(mutex: *mut c_void) -> *mut LegacyMutex {
    mutex.cast()
}

fn legacy_mutex_create(recursive: bool) -> *mut c_void {
    let mutex = LegacyMutex {
        locking_pid: usize::MAX,
        count: 0,
        recursive,
    };
    let ptr = unsafe { malloc(size_of_val(&mutex)) as *mut LegacyMutex };
    unsafe {
        ptr.write(mutex);
    }
    memory_fence();
    trace!("legacy_mutex_create recursive={} -> {:?}", recursive, ptr);
    ptr.cast()
}

fn legacy_mutex_delete(mutex: *mut c_void) {
    unsafe { free(mutex.cast()) };
}

fn legacy_mutex_lock(mutex: *mut c_void) -> i32 {
    let ptr = legacy_mutex_ptr(mutex);
    let current_task = crate::preempt::current_task() as usize;

    loop {
        let mutex_locked = ESP_RADIO_LOCK.lock(|| unsafe {
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
        memory_fence();

        if mutex_locked {
            return 1;
        }

        crate::preempt::yield_task();
    }
}

fn legacy_mutex_unlock(mutex: *mut c_void) -> i32 {
    let ptr = legacy_mutex_ptr(mutex);
    ESP_RADIO_LOCK.lock(|| unsafe {
        memory_fence();
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

pub(crate) fn mutex_create(recursive: bool) -> *mut c_void {
    if legacy_mutex_enabled() {
        return legacy_mutex_create(recursive);
    }

    let ptr = SemaphoreHandle::new(if recursive {
        SemaphoreKind::RecursiveMutex
    } else {
        SemaphoreKind::Mutex
    })
    .leak()
    .as_ptr()
    .cast();

    trace!("mutex_create -> {:?}", ptr);
    ptr
}

pub(crate) fn mutex_delete(mutex: *mut c_void) {
    if legacy_mutex_enabled() {
        legacy_mutex_delete(mutex);
        return;
    }

    let ptr = unwrap!(SemaphorePtr::new(mutex.cast()), "mutex is null");

    let handle = unsafe { SemaphoreHandle::from_ptr(ptr) };
    core::mem::drop(handle);
}

pub(crate) fn mutex_lock(mutex: *mut c_void) -> i32 {
    if legacy_mutex_enabled() {
        return legacy_mutex_lock(mutex);
    }

    let ptr = unwrap!(SemaphorePtr::new(mutex.cast()), "mutex is null");

    let handle = unsafe { SemaphoreHandle::ref_from_ptr(&ptr) };

    handle.take(None) as i32
}

pub(crate) fn mutex_unlock(mutex: *mut c_void) -> i32 {
    if legacy_mutex_enabled() {
        return legacy_mutex_unlock(mutex);
    }

    let ptr = unwrap!(SemaphorePtr::new(mutex.cast()), "mutex is null");

    let handle = unsafe { SemaphoreHandle::ref_from_ptr(&ptr) };

    handle.give() as i32
}
