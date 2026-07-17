use core::ffi::c_void;

use super::{
    internal_legacy_common_literal as legacy_common,
    internal_legacy_timer_backend as legacy_timers,
    os_adapter::legacy_interrupt_backend as legacy_interrupts,
};
#[cfg(coex)]
use crate::{binary::include::ets_timer, hal::ram};

#[cfg(coex)]
#[ram]
pub(super) unsafe extern "C" fn xtal_freq_get() -> i32 {
    use esp_hal::clock::Clock;

    let xtal = crate::hal::clock::RtcClock::xtal_freq();
    xtal.mhz() as i32
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn debug_matrix_init(
    _evt: i32,
    _sig: i32,
    _rev: bool,
) -> i32 {
    esp_wifi_sys::include::ESP_ERR_NOT_SUPPORTED as i32
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn is_in_isr() -> i32 {
    crate::is_interrupts_disabled() as i32
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn task_yield_from_isr() {
    unsafe { legacy_common::task_yield_from_isr() };
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn semphr_create(max: u32, init: u32) -> *mut c_void {
    unsafe { legacy_common::semphr_create(max, init) }
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn semphr_delete(semphr: *mut c_void) {
    unsafe { legacy_common::semphr_delete(semphr) };
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn semphr_take(semphr: *mut c_void, tick: u32) -> i32 {
    unsafe { legacy_common::semphr_take(semphr, tick) }
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn semphr_give(semphr: *mut c_void) -> i32 {
    unsafe { legacy_common::semphr_give(semphr) }
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn semphr_take_from_isr(
    semphr: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    unsafe { legacy_common::semphr_take_from_isr(semphr, higher_priority_task_waken.cast()) }
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn semphr_give_from_isr(
    semphr: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    unsafe { legacy_common::semphr_give_from_isr(semphr, higher_priority_task_waken.cast()) }
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    unsafe { legacy_common::malloc(size) }
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn free(ptr: *mut c_void) {
    unsafe { legacy_common::free(ptr) }
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn esp_timer_get_time() -> i64 {
    unsafe { legacy_common::esp_timer_get_time() }
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn timer_disarm(timer: *mut c_void) {
    unsafe { legacy_timers::timer_disarm(timer.cast::<ets_timer>()) };
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn timer_done(timer: *mut c_void) {
    unsafe { legacy_timers::timer_done(timer.cast::<ets_timer>()) };
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn timer_setfn(
    ptimer: *mut c_void,
    pfunction: *mut c_void,
    parg: *mut c_void,
) {
    unsafe {
        legacy_timers::timer_setfn(
            ptimer.cast::<ets_timer>(),
            core::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void)>(pfunction),
            parg,
        )
    };
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn timer_arm_us(
    timer: *mut c_void,
    us: u32,
    repeat: bool,
) {
    unsafe { legacy_timers::timer_arm_us(timer.cast::<ets_timer>(), us, repeat) };
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn env_is_chip() -> bool {
    unsafe { legacy_interrupts::env_is_chip() }
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn spin_lock_create() -> *mut c_void {
    unsafe { legacy_interrupts::spin_lock_create() }
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn spin_lock_delete(lock: *mut c_void) {
    unsafe { legacy_interrupts::spin_lock_delete(lock) };
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn wifi_int_disable(wifi_int_mux: *mut c_void) -> u32 {
    unsafe { legacy_interrupts::wifi_int_disable(wifi_int_mux) }
}

#[cfg(coex)]
pub(super) unsafe extern "C" fn wifi_int_restore(tmp: u32) {
    unsafe { legacy_interrupts::wifi_int_restore(tmp) };
}
