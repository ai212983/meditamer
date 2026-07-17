use crate::binary::c_types::{c_char, c_int, c_void};
use crate::hal::clock::ModemClockController;
use crate::hal::peripherals::WIFI;
use crate::wifi::os_adapter::os_adapter_chip_specific;

use super::internal_legacy_common_literal as legacy_common;

pub(crate) unsafe extern "C" fn dport_stall_start() {
    unsafe { super::dport_access_stall_other_cpu_start_wrap() };
}

pub(crate) unsafe extern "C" fn dport_stall_end() {
    unsafe { super::dport_access_stall_other_cpu_end_wrap() };
}

pub(crate) unsafe extern "C" fn apb80m_request() {}

pub(crate) unsafe extern "C" fn apb80m_release() {}

pub(crate) unsafe extern "C" fn phy_disable_legacy() {
    #[cfg(esp32)]
    unsafe {
        super::phy_legacy_esp32::phy_disable();
    }
    #[cfg(not(esp32))]
    unsafe {
        os_adapter_chip_specific::phy_disable();
    }
}

pub(crate) unsafe extern "C" fn phy_enable_legacy() {
    #[cfg(esp32)]
    unsafe {
        super::phy_legacy_esp32::phy_enable();
    }
    #[cfg(not(esp32))]
    unsafe {
        os_adapter_chip_specific::phy_enable();
    }
}

#[allow(clippy::unnecessary_cast)]
pub(crate) unsafe extern "C" fn phy_update_country_info_legacy(
    _country: *const c_char,
) -> c_int {
    -1
}

pub(crate) unsafe extern "C" fn read_mac_legacy(mac: *mut u8, type_: u32) -> c_int {
    unsafe { crate::common_adapter::read_mac(mac, type_) }
}

pub(crate) unsafe extern "C" fn wifi_reset_mac_legacy() {
    unsafe { WIFI::steal() }.reset_wifi_mac();
}

pub(crate) unsafe extern "C" fn wifi_clock_enable_legacy() {
    unsafe { WIFI::steal() }.enable_modem_clock(true);
}

pub(crate) unsafe extern "C" fn wifi_clock_disable_legacy() {
    unsafe { WIFI::steal() }.enable_modem_clock(false);
}

pub(crate) unsafe extern "C" fn wifi_rtc_enable_iso_legacy() {
    todo!("wifi_rtc_enable_iso")
}

pub(crate) unsafe extern "C" fn wifi_rtc_disable_iso_legacy() {
    todo!("wifi_rtc_disable_iso")
}

pub(crate) unsafe extern "C" fn nvs_set_i8_legacy(
    _handle: u32,
    _key: *const c_char,
    _value: i8,
) -> c_int {
    todo!("nvs_set_i8")
}

pub(crate) unsafe extern "C" fn nvs_get_i8_legacy(
    _handle: u32,
    _key: *const c_char,
    _out_value: *mut i8,
) -> c_int {
    todo!("nvs_get_i8")
}

pub(crate) unsafe extern "C" fn nvs_set_u8_legacy(
    _handle: u32,
    _key: *const c_char,
    _value: u8,
) -> c_int {
    todo!("nvs_set_u8")
}

pub(crate) unsafe extern "C" fn nvs_get_u8_legacy(
    _handle: u32,
    _key: *const c_char,
    _out_value: *mut u8,
) -> c_int {
    todo!("nvs_get_u8")
}

pub(crate) unsafe extern "C" fn nvs_set_u16_legacy(
    _handle: u32,
    _key: *const c_char,
    _value: u16,
) -> c_int {
    todo!("nvs_set_u16")
}

pub(crate) unsafe extern "C" fn nvs_get_u16_legacy(
    _handle: u32,
    _key: *const c_char,
    _out_value: *mut u16,
) -> c_int {
    todo!("nvs_get_u16")
}

pub(crate) unsafe extern "C" fn nvs_open_legacy(
    _name: *const c_char,
    _open_mode: u32,
    _out_handle: *mut u32,
) -> c_int {
    todo!("nvs_open")
}

pub(crate) unsafe extern "C" fn nvs_close_legacy(_handle: u32) {
    todo!("nvs_close")
}

pub(crate) unsafe extern "C" fn nvs_commit_legacy(_handle: u32) -> c_int {
    todo!("nvs_commit")
}

pub(crate) unsafe extern "C" fn nvs_set_blob_legacy(
    _handle: u32,
    _key: *const c_char,
    _value: *const c_void,
    _length: usize,
) -> c_int {
    todo!("nvs_set_blob")
}

pub(crate) unsafe extern "C" fn nvs_get_blob_legacy(
    _handle: u32,
    _key: *const c_char,
    _out_value: *mut c_void,
    _length: *mut usize,
) -> c_int {
    todo!("nvs_get_blob")
}

pub(crate) unsafe extern "C" fn nvs_erase_key_legacy(
    _handle: u32,
    _key: *const c_char,
) -> c_int {
    todo!("nvs_erase_key")
}

pub(crate) unsafe extern "C" fn malloc_internal(size: usize) -> *mut c_void {
    unsafe { legacy_common::malloc(size) }
}
