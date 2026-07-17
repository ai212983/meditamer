use crate::binary::{
    c_types::{c_char, c_int, c_void},
    include::coex_schm_interval_get,
};

use super::{
    dport_access_stall_other_cpu_end_wrap,
    dport_access_stall_other_cpu_start_wrap,
    nvs_close,
    nvs_commit,
    nvs_erase_key,
    nvs_get_blob,
    nvs_get_i8,
    nvs_get_u8,
    nvs_get_u16,
    nvs_open,
    nvs_set_blob,
    nvs_set_i8,
    nvs_set_u8,
    nvs_set_u16,
    phy_update_country_info,
    wifi_apb80m_release,
    wifi_apb80m_request,
    wifi_clock_disable,
    wifi_clock_enable,
    wifi_reset_mac,
    wifi_rtc_disable_iso,
    wifi_rtc_enable_iso,
};
use crate::{
    common_adapter::read_mac,
    wifi::phy_legacy_esp32,
};

pub(crate) unsafe extern "C" fn dport_stall_start() {
    unsafe { dport_access_stall_other_cpu_start_wrap() };
}

pub(crate) unsafe extern "C" fn dport_stall_end() {
    unsafe { dport_access_stall_other_cpu_end_wrap() };
}

pub(crate) unsafe extern "C" fn apb80m_request() {
    unsafe { wifi_apb80m_request() };
}

pub(crate) unsafe extern "C" fn apb80m_release() {
    unsafe { wifi_apb80m_release() };
}

pub(crate) unsafe extern "C" fn phy_disable_legacy() {
    #[cfg(esp32)]
    unsafe {
        phy_legacy_esp32::phy_disable();
    }
    #[cfg(not(esp32))]
    unsafe {
        super::phy_disable();
    }
}

pub(crate) unsafe extern "C" fn phy_enable_legacy() {
    #[cfg(esp32)]
    unsafe {
        phy_legacy_esp32::phy_enable();
    }
    #[cfg(not(esp32))]
    unsafe {
        super::phy_enable();
    }
}

pub(crate) unsafe extern "C" fn phy_update_country_info_legacy(country: *const c_char) -> c_int {
    unsafe { phy_update_country_info(country) }
}

pub(crate) unsafe extern "C" fn read_mac_legacy(mac: *mut u8, type_: u32) -> c_int {
    unsafe { read_mac(mac, type_) }
}

pub(crate) unsafe extern "C" fn timer_schm_interval_get_legacy() -> u32 {
    unsafe { coex_schm_interval_get() }
}

pub(crate) unsafe extern "C" fn wifi_reset_mac_legacy() {
    unsafe { wifi_reset_mac() };
}

pub(crate) unsafe extern "C" fn wifi_clock_enable_legacy() {
    unsafe { wifi_clock_enable() };
}

pub(crate) unsafe extern "C" fn wifi_clock_disable_legacy() {
    unsafe { wifi_clock_disable() };
}

pub(crate) unsafe extern "C" fn wifi_rtc_enable_iso_legacy() {
    unsafe { wifi_rtc_enable_iso() };
}

pub(crate) unsafe extern "C" fn wifi_rtc_disable_iso_legacy() {
    unsafe { wifi_rtc_disable_iso() };
}

pub(crate) unsafe extern "C" fn nvs_set_i8_legacy(handle: u32, key: *const c_char, value: i8) -> c_int {
    unsafe { nvs_set_i8(handle, key, value) }
}

pub(crate) unsafe extern "C" fn nvs_get_i8_legacy(
    handle: u32,
    key: *const c_char,
    out_value: *mut i8,
) -> c_int {
    unsafe { nvs_get_i8(handle, key, out_value) }
}

pub(crate) unsafe extern "C" fn nvs_set_u8_legacy(handle: u32, key: *const c_char, value: u8) -> c_int {
    unsafe { nvs_set_u8(handle, key, value) }
}

pub(crate) unsafe extern "C" fn nvs_get_u8_legacy(
    handle: u32,
    key: *const c_char,
    out_value: *mut u8,
) -> c_int {
    unsafe { nvs_get_u8(handle, key, out_value) }
}

pub(crate) unsafe extern "C" fn nvs_set_u16_legacy(handle: u32, key: *const c_char, value: u16) -> c_int {
    unsafe { nvs_set_u16(handle, key, value) }
}

pub(crate) unsafe extern "C" fn nvs_get_u16_legacy(
    handle: u32,
    key: *const c_char,
    out_value: *mut u16,
) -> c_int {
    unsafe { nvs_get_u16(handle, key, out_value) }
}

pub(crate) unsafe extern "C" fn nvs_open_legacy(
    name: *const c_char,
    open_mode: u32,
    out_handle: *mut u32,
) -> c_int {
    unsafe { nvs_open(name, open_mode, out_handle) }
}

pub(crate) unsafe extern "C" fn nvs_close_legacy(handle: u32) {
    unsafe { nvs_close(handle) };
}

pub(crate) unsafe extern "C" fn nvs_commit_legacy(handle: u32) -> c_int {
    unsafe { nvs_commit(handle) }
}

pub(crate) unsafe extern "C" fn nvs_set_blob_legacy(
    handle: u32,
    key: *const c_char,
    value: *const c_void,
    length: usize,
) -> c_int {
    unsafe { nvs_set_blob(handle, key, value, length) }
}

pub(crate) unsafe extern "C" fn nvs_get_blob_legacy(
    handle: u32,
    key: *const c_char,
    out_value: *mut c_void,
    length: *mut usize,
) -> c_int {
    unsafe { nvs_get_blob(handle, key, out_value, length) }
}

pub(crate) unsafe extern "C" fn nvs_erase_key_legacy(handle: u32, key: *const c_char) -> c_int {
    unsafe { nvs_erase_key(handle, key) }
}
