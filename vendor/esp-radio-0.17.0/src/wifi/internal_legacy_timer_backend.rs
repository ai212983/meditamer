use core::ffi::c_void;

use esp_wifi_sys::include::ets_timer;

use crate::compat::timer_compat_legacy;

pub(super) unsafe extern "C" fn timer_disarm(timer: *mut c_void) {
    timer_compat_legacy::compat_timer_disarm(timer.cast::<ets_timer>());
}

pub(super) unsafe extern "C" fn timer_done(timer: *mut c_void) {
    timer_compat_legacy::compat_timer_done(timer.cast::<ets_timer>());
}

pub(super) unsafe extern "C" fn timer_setfn(
    ptimer: *mut c_void,
    pfunction: *mut c_void,
    parg: *mut c_void,
) {
    timer_compat_legacy::compat_timer_setfn(
        ptimer.cast::<ets_timer>(),
        unsafe {
            core::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void)>(pfunction)
        },
        parg,
    );
}

pub(super) unsafe extern "C" fn timer_arm(timer: *mut c_void, tmout_ms: u32, repeat: bool) {
    timer_compat_legacy::compat_timer_arm(timer.cast::<ets_timer>(), tmout_ms, repeat);
}

pub(super) unsafe extern "C" fn timer_arm_us(timer: *mut c_void, us: u32, repeat: bool) {
    timer_compat_legacy::compat_timer_arm_us(timer.cast::<ets_timer>(), us, repeat);
}
