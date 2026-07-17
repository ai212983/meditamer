use core::ffi::c_void;

use super::{
    internal_legacy_common_literal as legacy_common,
    internal_legacy_timer_backend as legacy_timers,
    os_adapter::legacy_interrupt_backend as legacy_interrupts,
    slowclk_cal_get,
};

#[cfg(all(coex, any(esp32, esp32c2, esp32c3, esp32c6, esp32s3)))]
pub(super) static mut LEGACY_G_COEX_ADAPTER_FUNCS: crate::binary::include::coex_adapter_funcs_t =
    crate::binary::include::coex_adapter_funcs_t {
        _version: crate::binary::include::COEX_ADAPTER_VERSION as i32,
        _task_yield_from_isr: Some(legacy_common::task_yield_from_isr),
        _semphr_create: Some(legacy_common::semphr_create),
        _semphr_delete: Some(legacy_common::semphr_delete),
        _semphr_take_from_isr: Some(legacy_common::semphr_take_from_isr_c_void),
        _semphr_give_from_isr: Some(legacy_common::semphr_give_from_isr_c_void),
        _semphr_take: Some(legacy_common::semphr_take),
        _semphr_give: Some(legacy_common::semphr_give),
        _is_in_isr: Some(is_in_isr),
        _malloc_internal: Some(legacy_common::malloc),
        _free: Some(legacy_common::free),
        _esp_timer_get_time: Some(legacy_common::esp_timer_get_time),
        _env_is_chip: Some(legacy_interrupts::env_is_chip),
        _magic: crate::binary::include::COEX_ADAPTER_MAGIC as i32,
        _timer_disarm: Some(timer_disarm),
        _timer_done: Some(timer_done),
        _timer_setfn: Some(timer_setfn),
        _timer_arm_us: Some(timer_arm_us),

        #[cfg(esp32)]
        _spin_lock_create: Some(legacy_interrupts::spin_lock_create),
        #[cfg(esp32)]
        _spin_lock_delete: Some(legacy_interrupts::spin_lock_delete),
        #[cfg(esp32)]
        _int_disable: Some(legacy_interrupts::wifi_int_disable),
        #[cfg(esp32)]
        _int_enable: Some(legacy_interrupts::wifi_int_restore),

        #[cfg(esp32c2)]
        _slowclk_cal_get: Some(slowclk_cal_get),

        _debug_matrix_init: Some(debug_matrix_init),
        _xtal_freq_get: Some(xtal_freq_get),
    };

pub(super) unsafe extern "C" fn coex_init() -> i32 {
    #[cfg(coex)]
    {
        return unsafe { crate::binary::include::coex_init() };
    }
    #[cfg(not(coex))]
    0
}

pub(super) unsafe extern "C" fn coex_deinit() {
    #[cfg(coex)]
    unsafe { crate::binary::include::coex_deinit() };
}

pub(super) unsafe extern "C" fn coex_enable() -> i32 {
    #[cfg(coex)]
    {
        return unsafe { crate::binary::include::coex_enable() };
    }
    #[cfg(not(coex))]
    0
}

pub(super) unsafe extern "C" fn coex_disable() {
    #[cfg(coex)]
    unsafe { crate::binary::include::coex_disable() };
}

pub(super) unsafe extern "C" fn coex_wifi_request(event: u32, latency: u32, duration: u32) -> i32 {
    #[cfg(coex)]
    {
        return unsafe { crate::binary::include::coex_wifi_request(event, latency, duration) };
    }
    #[cfg(not(coex))]
    0
}

pub(super) unsafe extern "C" fn coex_wifi_release(event: u32) -> i32 {
    #[cfg(coex)]
    {
        return unsafe { crate::binary::include::coex_wifi_release(event) };
    }
    #[cfg(not(coex))]
    0
}

pub(super) unsafe extern "C" fn coex_wifi_channel_set(primary: u8, secondary: u8) -> i32 {
    #[cfg(coex)]
    {
        return unsafe { crate::binary::include::coex_wifi_channel_set(primary, secondary) };
    }
    #[cfg(not(coex))]
    0
}

pub(super) unsafe extern "C" fn coex_event_duration_get(event: u32, duration: *mut u32) -> i32 {
    #[cfg(coex)]
    {
        return unsafe { crate::binary::include::coex_event_duration_get(event, duration) };
    }
    #[cfg(not(coex))]
    0
}

pub(super) unsafe extern "C" fn coex_pti_get(event: u32, pti: *mut u8) -> i32 {
    #[cfg(coex)]
    {
        return unsafe { crate::binary::include::coex_pti_get(event, pti) };
    }
    #[cfg(not(coex))]
    0
}

pub(super) unsafe extern "C" fn coex_schm_status_bit_clear(type_: u32, status: u32) {
    #[cfg(coex)]
    unsafe { crate::binary::include::coex_schm_status_bit_clear(type_, status) };
}

pub(super) unsafe extern "C" fn coex_schm_status_bit_set(type_: u32, status: u32) {
    #[cfg(coex)]
    unsafe { crate::binary::include::coex_schm_status_bit_set(type_, status) };
}

pub(super) unsafe extern "C" fn coex_schm_interval_set(interval: u32) -> i32 {
    #[cfg(coex)]
    {
        return unsafe { crate::binary::include::coex_schm_interval_set(interval) };
    }
    #[cfg(not(coex))]
    0
}

pub(super) unsafe extern "C" fn coex_schm_interval_get() -> u32 {
    #[cfg(coex)]
    {
        return unsafe { crate::binary::include::coex_schm_interval_get() };
    }
    #[cfg(not(coex))]
    0
}

pub(super) unsafe extern "C" fn coex_schm_curr_period_get() -> u8 {
    #[cfg(coex)]
    {
        return unsafe { crate::binary::include::coex_schm_curr_period_get() };
    }
    #[cfg(not(coex))]
    0
}

pub(super) unsafe extern "C" fn coex_schm_curr_phase_get() -> *mut c_void {
    #[cfg(coex)]
    {
        return unsafe { crate::binary::include::coex_schm_curr_phase_get() };
    }
    #[cfg(not(coex))]
    core::ptr::null_mut()
}

pub(super) unsafe extern "C" fn coex_register_start_cb(
    cb: Option<unsafe extern "C" fn() -> esp_wifi_sys::c_types::c_int>,
) -> i32 {
    #[cfg(coex)]
    {
        return unsafe { esp_wifi_sys::include::coex_register_start_cb(cb) };
    }
    #[cfg(not(coex))]
    0
}

pub(super) unsafe extern "C" fn coex_schm_process_restart() -> i32 {
    #[cfg(coex)]
    {
        return unsafe { crate::binary::include::coex_schm_process_restart() };
    }
    #[cfg(not(coex))]
    0
}

pub(super) unsafe extern "C" fn coex_schm_register_cb(
    arg1: esp_wifi_sys::c_types::c_int,
    cb: ::core::option::Option<
        unsafe extern "C" fn(arg1: esp_wifi_sys::c_types::c_int) -> esp_wifi_sys::c_types::c_int,
    >,
) -> i32 {
    #[cfg(coex)]
    {
        return unsafe {
            crate::binary::include::coex_schm_register_callback(
                arg1 as u32,
                unwrap!(cb) as *const esp_wifi_sys::c_types::c_void
                    as *mut esp_wifi_sys::c_types::c_void,
            )
        };
    }
    #[cfg(not(coex))]
    0
}

pub(super) unsafe extern "C" fn coex_schm_flexible_period_set(period: u8) -> i32 {
    #[cfg(coex)]
    {
        unsafe extern "C" {
            fn coex_schm_flexible_period_set(period: u8) -> i32;
        }
        return unsafe { coex_schm_flexible_period_set(period) };
    }
    #[cfg(not(coex))]
    0
}

pub(super) unsafe extern "C" fn coex_schm_flexible_period_get() -> u8 {
    #[cfg(coex)]
    {
        unsafe extern "C" {
            fn coex_schm_flexible_period_get() -> u8;
        }
        return unsafe { coex_schm_flexible_period_get() };
    }
    #[cfg(not(coex))]
    0
}

pub(super) unsafe extern "C" fn coex_schm_get_phase_by_idx(idx: i32) -> *mut c_void {
    #[cfg(coex)]
    {
        return unsafe { crate::binary::include::coex_schm_get_phase_by_idx(idx) };
    }
    #[cfg(not(coex))]
    core::ptr::null_mut()
}

#[cfg(coex)]
unsafe extern "C" fn timer_arm_us(timer: *mut c_void, us: u32, repeat: bool) {
    timer_compat_legacy::compat_timer_arm_us(timer.cast::<ets_timer>(), us, repeat);
}
