#![allow(dead_code)]

pub mod common;
pub(crate) mod common_adapter_legacy_backend;
pub(crate) mod common_adapter_legacy_osi_backend;
pub(crate) mod common_adapter_legacy_queue_backend;
pub(crate) mod common_legacy_queue;
pub(crate) mod common_legacy_backend;
pub(crate) mod common_legacy_literal;
pub(crate) mod common_legacy;
pub(crate) mod common_legacy_port;
#[cfg(any(feature = "wifi", all(feature = "ble", bt_controller = "npl")))]
pub(crate) mod legacy_wifi_tasks;
pub(crate) mod preempt_legacy_backend;
pub mod malloc;
pub(crate) mod legacy_runtime_policy;
pub mod misc;
pub mod mutex;
pub mod queue;
pub(crate) mod queue_legacy;
pub(crate) mod queue_legacy_backend;
pub mod semaphore;

#[cfg(any(feature = "wifi", all(feature = "ble", bt_controller = "npl")))]
pub mod timer_compat;
#[cfg(any(feature = "wifi", all(feature = "ble", bt_controller = "npl")))]
pub(crate) mod timer_compat_legacy;
#[cfg(any(feature = "wifi", all(feature = "ble", bt_controller = "npl")))]
pub(crate) mod timer_compat_legacy_diag;
#[cfg(any(feature = "wifi", all(feature = "ble", bt_controller = "npl")))]
pub(crate) mod timer_compat_legacy_policy;

pub(crate) const OSI_FUNCS_TIME_BLOCKING: u32 = u32::MAX;

#[unsafe(no_mangle)]
unsafe extern "C" fn __esp_radio_putchar(_c: u8) {
    #[cfg(feature = "sys-logs")]
    {
        static mut BUFFER: [u8; 256] = [0u8; 256];
        static mut IDX: usize = 0;

        unsafe {
            let buffer = core::ptr::addr_of_mut!(BUFFER);
            if _c == 0 || _c == b'\n' || IDX == (*buffer).len() - 1 {
                if _c != 0 {
                    BUFFER[IDX] = _c;
                } else {
                    IDX = IDX.saturating_sub(1);
                }

                info!("{}", core::str::from_utf8_unchecked(&BUFFER[..IDX]));
                IDX = 0;
            } else {
                BUFFER[IDX] = _c;
                IDX += 1;
            }
        }
    }
}
