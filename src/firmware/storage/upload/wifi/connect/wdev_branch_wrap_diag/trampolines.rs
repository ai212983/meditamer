use super::*;
use super::rx::{RxDirectSnapshot, WDEV_PROCESS_RX_DIRECT_RING};

unsafe extern "C" {
    fn __real_wdev_process_panic_watchdog() -> usize;
    #[link_name = "wdev_process_panic_watchdog"]
    fn wdev_process_panic_watchdog_direct() -> usize;
    #[link_name = "lmacProcessRxSucData"]
    fn lmac_process_rx_suc_data_direct() -> usize;
    #[link_name = "wDev_ProcessRxSucData"]
    fn wdev_process_rx_suc_data_direct(a2: usize, a3: usize, a4: usize, a5: usize) -> usize;
    #[link_name = "pp_post"]
    fn pp_post_direct(a2: usize, a3: usize) -> usize;
    fn __real_hal_mac_interrupt_get_event() -> usize;
    fn __real_hal_mac_interrupt_clr_event(a2: usize) -> usize;
    fn __real_hal_mac_rx_get_end_info(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_pp_post(a2: usize, a3: usize) -> usize;
    fn __real_lmacProcessRxSucData() -> usize;
}

#[used]
static KEEP_WDEV_PANIC_WATCHDOG_TRAMPOLINE: unsafe extern "C" fn() -> usize =
    wdev_process_panic_watchdog_trampoline;

#[used]
static KEEP_LMAC_RX_SUC_TRAMPOLINE: unsafe extern "C" fn() -> usize =
    lmac_process_rx_suc_data_trampoline;

#[used]
static KEEP_PP_POST_TRAMPOLINE: unsafe extern "C" fn(usize, usize) -> usize = pp_post_trampoline;

#[used]
static KEEP_WDEV_PROCESS_RX_SUC_TRAMPOLINE: unsafe extern "C" fn(usize, usize, usize, usize) -> usize =
    wdev_process_rx_suc_data_trampoline;

#[used]
#[unsafe(no_mangle)]
pub static WDEV_BRANCH_TRAMPOLINES_KEEP: [unsafe extern "C" fn(); 4] = [
    wdev_branch_trampoline_keep_watchdog,
    wdev_branch_trampoline_keep_lmac,
    wdev_branch_trampoline_keep_pp_post,
    wdev_branch_trampoline_keep_wdev_rx,
];

#[inline(never)]
unsafe extern "C" fn wdev_branch_trampoline_keep_watchdog() {
    let func = wdev_process_panic_watchdog_trampoline as *const ();
    unsafe { core::ptr::read_volatile(&func) };
}

#[inline(never)]
unsafe extern "C" fn wdev_branch_trampoline_keep_lmac() {
    let func = lmac_process_rx_suc_data_trampoline as *const ();
    unsafe { core::ptr::read_volatile(&func) };
}

#[inline(never)]
unsafe extern "C" fn wdev_branch_trampoline_keep_pp_post() {
    let func = pp_post_trampoline as *const ();
    unsafe { core::ptr::read_volatile(&func) };
}

#[inline(never)]
unsafe extern "C" fn wdev_branch_trampoline_keep_wdev_rx() {
    let func = wdev_process_rx_suc_data_trampoline as *const ();
    unsafe { core::ptr::read_volatile(&func) };
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".wifi0iram")]
pub(super) unsafe extern "C" fn wdev_process_panic_watchdog_trampoline() -> usize {
    let pre_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { wdev_process_panic_watchdog_direct() };
    let post_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    PANIC_WATCHDOG_RING.push(Snapshot {
        arg0: 0,
        arg1: 0,
        ret: ret as u32,
        ret_forced: ret as u32,
        pre_mac_isr,
        post_mac_isr,
        pre_rx_sta: pre_rx_sta as u32,
        post_rx_sta: post_rx_sta as u32,
    });
    ret
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".wifi0iram")]
pub(super) unsafe extern "C" fn lmac_process_rx_suc_data_trampoline() -> usize {
    let pre_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { lmac_process_rx_suc_data_direct() };
    let post_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    LMAC_RX_SUC_RING.push(Snapshot {
        arg0: 0,
        arg1: 0,
        ret: ret as u32,
        ret_forced: ret as u32,
        pre_mac_isr,
        post_mac_isr,
        pre_rx_sta: pre_rx_sta as u32,
        post_rx_sta: post_rx_sta as u32,
    });
    ret
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".wifi0iram")]
pub(super) unsafe extern "C" fn pp_post_trampoline(a2: usize, a3: usize) -> usize {
    let pre_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { pp_post_direct(a2, a3) };
    if a2 == 25 {
        PP_POST_ARG25_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    let post_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    PP_POST_RING.push(Snapshot {
        arg0: a2 as u32,
        arg1: a3 as u32,
        ret: ret as u32,
        ret_forced: ret as u32,
        pre_mac_isr,
        post_mac_isr,
        pre_rx_sta: pre_rx_sta as u32,
        post_rx_sta: post_rx_sta as u32,
    });
    ret
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".wifi0iram")]
pub(super) unsafe extern "C" fn wdev_process_rx_suc_data_trampoline(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
) -> usize {
    let ret = unsafe { wdev_process_rx_suc_data_direct(a2, a3, a4, a5) };
    WDEV_PROCESS_RX_DIRECT_RING.push(RxDirectSnapshot {
        args: [a2 as u32, a3 as u32, a4 as u32, a5 as u32],
        ret: ret as u32,
        pre_mac_isr: 0,
        post_mac_isr: 0,
        pre_rx_sta: 0,
        post_rx_sta: 0,
    });
    ret
}
