use alloc::boxed::Box;
use core::ptr::NonNull;

use esp_hal::time::{Duration, Instant};
use esp_radio_rtos_driver::semaphore::SemaphoreKind;
use esp_wifi_sys::c_types::c_void;

use crate::{
    compat::OSI_FUNCS_TIME_BLOCKING,
    ESP_RADIO_LOCK,
    memory_fence::memory_fence,
    preempt::semaphore::{SemaphoreHandle, SemaphorePtr},
};

fn legacy_simple_sem_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_LEGACY_SIMPLE_SEM_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn legacy_sem_ptr(semphr: *mut c_void) -> *mut u32 {
    let ptr = NonNull::new(semphr.cast::<u32>()).expect("semphr is null");
    ptr.as_ptr()
}

pub(crate) fn sem_create(max: u32, initial: u32) -> *mut c_void {
    if legacy_simple_sem_enabled() {
        let ptr = Box::into_raw(Box::new(initial));
        trace!("sem_create legacy -> {:?}", ptr);
        return ptr.cast();
    }

    let ptr = SemaphoreHandle::new(SemaphoreKind::Counting { max, initial })
        .leak()
        .as_ptr()
        .cast();

    trace!("sem_create -> {:?}", ptr);

    ptr
}

pub(crate) fn sem_delete(semphr: *mut c_void) {
    trace!("sem_delete: {:?}", semphr);

    if legacy_simple_sem_enabled() {
        let ptr = legacy_sem_ptr(semphr);
        unsafe { drop(Box::from_raw(ptr)) };
        return;
    }

    let ptr = unwrap!(SemaphorePtr::new(semphr.cast()), "semphr is null");

    let handle = unsafe { SemaphoreHandle::from_ptr(ptr) };
    core::mem::drop(handle);
}

pub(crate) fn sem_take(semphr: *mut c_void, tick: u32) -> i32 {
    if legacy_simple_sem_enabled() {
        let forever = tick == OSI_FUNCS_TIME_BLOCKING;
        let deadline = Instant::now() + Duration::from_micros(tick as u64);
        let sem = legacy_sem_ptr(semphr);

        loop {
            let res = ESP_RADIO_LOCK.lock(|| unsafe {
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
            if !forever && deadline < Instant::now() {
                return 0;
            }
            crate::preempt::yield_task();
        }
    }

    let ptr = unwrap!(SemaphorePtr::new(semphr.cast()), "semphr is null");

    let handle = unsafe { SemaphoreHandle::ref_from_ptr(&ptr) };
    // Assuming `tick` is in microseconds
    let timeout = if tick == OSI_FUNCS_TIME_BLOCKING {
        None
    } else {
        Some(tick)
    };

    handle.take(timeout) as i32
}

pub(crate) fn sem_try_take_from_isr(semphr: *mut c_void, higher_prio_task_waken: *mut bool) -> i32 {
    if legacy_simple_sem_enabled() {
        let sem = legacy_sem_ptr(semphr);
        return ESP_RADIO_LOCK.lock(|| unsafe {
            let cnt = *sem;
            if cnt > 0 {
                *sem = cnt - 1;
                if let Some(waken) = higher_prio_task_waken.as_mut() {
                    *waken = true;
                }
                1
            } else {
                0
            }
        });
    }

    let ptr = unwrap!(SemaphorePtr::new(semphr.cast()), "semphr is null");

    let handle = unsafe { SemaphoreHandle::ref_from_ptr(&ptr) };

    handle.try_take_from_isr(unsafe { higher_prio_task_waken.as_mut() }) as i32
}

pub(crate) fn sem_give(semphr: *mut c_void) -> i32 {
    if legacy_simple_sem_enabled() {
        let sem = legacy_sem_ptr(semphr);
        return ESP_RADIO_LOCK.lock(|| unsafe {
            *sem = (*sem).saturating_add(1);
            1
        });
    }

    let ptr = unwrap!(SemaphorePtr::new(semphr.cast()), "semphr is null");

    let handle = unsafe { SemaphoreHandle::ref_from_ptr(&ptr) };

    handle.give() as i32
}

pub(crate) fn sem_try_give_from_isr(semphr: *mut c_void, higher_prio_task_waken: *mut bool) -> i32 {
    if legacy_simple_sem_enabled() {
        let sem = legacy_sem_ptr(semphr);
        return ESP_RADIO_LOCK.lock(|| unsafe {
            *sem = (*sem).saturating_add(1);
            if let Some(waken) = higher_prio_task_waken.as_mut() {
                *waken = true;
            }
            1
        });
    }

    let ptr = unwrap!(SemaphorePtr::new(semphr.cast()), "semphr is null");

    let handle = unsafe { SemaphoreHandle::ref_from_ptr(&ptr) };

    handle.try_give_from_isr(unsafe { higher_prio_task_waken.as_mut() }) as i32
}

pub(crate) fn sem_give_from_isr(semphr: *mut c_void, _higher_prio_task_waken: *mut bool) -> i32 {
    if legacy_simple_sem_enabled() {
        let sem = legacy_sem_ptr(semphr);
        return ESP_RADIO_LOCK.lock(|| unsafe {
            *sem = (*sem).saturating_add(1);
            1
        });
    }

    let ptr = unwrap!(SemaphorePtr::new(semphr.cast()), "semphr is null");

    let handle = unsafe { SemaphoreHandle::ref_from_ptr(&ptr) };

    handle.give() as i32
}
