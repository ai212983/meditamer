use core::ffi::c_void;

use crate::{
    compat::{
        malloc::{free, malloc},
        OSI_FUNCS_TIME_BLOCKING,
    },
    memory_fence::memory_fence,
    time,
};

pub(crate) unsafe fn event_group_create() -> *mut c_void {
    let ptr = unsafe { malloc(4) as *mut u32 };
    unsafe { ptr.write_volatile(0) };
    ptr.cast()
}

pub(crate) unsafe fn event_group_delete(event: *mut c_void) {
    unsafe { free(event.cast()) };
}

pub(crate) unsafe fn event_group_set_bits(event: *mut c_void, bits: u32) -> u32 {
    let group = event.cast::<u32>();
    critical_section::with(|_| unsafe {
        memory_fence();
        let current = *group;
        let updated = current | bits;
        *group = updated;
        updated
    })
}

pub(crate) unsafe fn event_group_clear_bits(event: *mut c_void, bits: u32) -> u32 {
    let group = event.cast::<u32>();
    critical_section::with(|_| unsafe {
        memory_fence();
        let current = *group;
        let updated = current & !bits;
        *group = updated;
        updated
    })
}

pub(crate) unsafe fn event_group_wait_bits(
    event: *mut c_void,
    bits_to_wait_for: u32,
    clear_on_exit: i32,
    wait_for_all_bits: i32,
    block_time_tick: u32,
) -> u32 {
    let forever = block_time_tick == OSI_FUNCS_TIME_BLOCKING;
    let timeout = block_time_tick as u64;
    let start = time::systimer_count();
    let group = event.cast::<u32>();

    loop {
        let matched = critical_section::with(|_| unsafe {
            memory_fence();
            let current = *group;
            let ready = if wait_for_all_bits != 0 {
                (current & bits_to_wait_for) == bits_to_wait_for
            } else {
                (current & bits_to_wait_for) != 0
            };

            if ready {
                let result = current;
                if clear_on_exit != 0 {
                    *group = current & !bits_to_wait_for;
                }
                Some(result)
            } else {
                None
            }
        });

        if let Some(result) = matched {
            return result;
        }

        if !forever && time::elapsed_time_since(start) > timeout {
            return critical_section::with(|_| unsafe { *group });
        }

        crate::compat::preempt_legacy_backend::yield_task();
    }
}
