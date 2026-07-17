use super::os_adapter_chip_specific;

#[cfg(feature = "sys-logs")]
pub(crate) unsafe extern "C" fn log_write_legacy(
    level: i32,
    tag: *const crate::binary::c_types::c_char,
    format: *const crate::binary::c_types::c_char,
    args: *const crate::binary::c_types::c_void,
) {
    unsafe { super::log_write(level, tag, format, args) }
}

#[cfg(feature = "sys-logs")]
pub(crate) unsafe extern "C" fn log_writev_legacy(
    level: i32,
    tag: *const crate::binary::c_types::c_char,
    format: *const crate::binary::c_types::c_char,
    args: *const crate::binary::c_types::c_void,
) {
    unsafe { super::log_writev(level, tag, format, args) }
}

#[cfg(any(esp32c3, esp32c2, esp32c6, esp32h2, esp32s3, esp32s2))]
pub(crate) unsafe extern "C" fn slowclk_cal_get_legacy() -> u32 {
    unsafe { crate::wifi::slowclk_cal_get() }
}

#[cfg(any(esp32, esp32s2))]
pub(crate) unsafe extern "C" fn phy_common_clock_disable_legacy() {
    unsafe { os_adapter_chip_specific::phy_common_clock_disable() }
}

#[cfg(any(esp32, esp32s2))]
pub(crate) unsafe extern "C" fn phy_common_clock_enable_legacy() {
    unsafe { os_adapter_chip_specific::phy_common_clock_enable() }
}
