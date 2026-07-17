use core::ffi::c_void;

use esp_sync::RawMutex;

use super::os_adapter_chip_specific;
use crate::compat::common_legacy_port;

static LEGACY_WIFI_LOCK: RawMutex = RawMutex::new();

pub(crate) unsafe extern "C" fn env_is_chip() -> bool {
    true
}

pub(crate) unsafe extern "C" fn set_intr(
    cpu_no: i32,
    intr_source: u32,
    intr_num: u32,
    intr_prio: i32,
) {
    unsafe {
        os_adapter_chip_specific::set_intr(cpu_no, intr_source, intr_num, intr_prio);
    }
}

pub(crate) unsafe extern "C" fn clear_intr(_intr_source: u32, _intr_num: u32) {
    // Legacy implementation intentionally does nothing.
}

pub(crate) unsafe extern "C" fn ints_on(mask: u32) {
    os_adapter_chip_specific::chip_ints_on(mask);
}

pub(crate) unsafe extern "C" fn ints_off(mask: u32) {
    os_adapter_chip_specific::chip_ints_off(mask);
}

pub(crate) unsafe extern "C" fn is_from_isr() -> bool {
    true
}

pub(crate) unsafe extern "C" fn set_isr(n: i32, f: *mut c_void, arg: *mut c_void) {
    unsafe { os_adapter_chip_specific::set_isr(n, f, arg) };
}

pub(crate) unsafe extern "C" fn spin_lock_create() -> *mut c_void {
    unsafe { common_legacy_port::semphr_create(1, 1) }
}

pub(crate) unsafe extern "C" fn spin_lock_delete(lock: *mut c_void) {
    unsafe { common_legacy_port::semphr_delete(lock) };
}

pub(crate) unsafe extern "C" fn wifi_int_disable(_wifi_int_mux: *mut c_void) -> u32 {
    let token = unsafe { LEGACY_WIFI_LOCK.acquire() };
    unsafe { core::mem::transmute::<esp_sync::RestoreState, u32>(token) }
}

pub(crate) unsafe extern "C" fn wifi_int_restore(_wifi_int_mux: *mut c_void, tmp: u32) {
    let token = unsafe { core::mem::transmute::<u32, esp_sync::RestoreState>(tmp) };
    unsafe { LEGACY_WIFI_LOCK.release(token) }
}
