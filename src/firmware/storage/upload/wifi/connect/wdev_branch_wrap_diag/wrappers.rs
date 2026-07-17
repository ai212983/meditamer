use super::*;
use super::api::read_event_window_words;

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

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_wdev_process_panic_watchdog() -> usize {
    let pre_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { __real_wdev_process_panic_watchdog() };
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
unsafe extern "C" fn __wrap_hal_mac_interrupt_get_event() -> usize {
    let pre_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    let pre_words = read_event_window_words();
    let ret = unsafe { __real_hal_mac_interrupt_get_event() };
    let ret_forced = if WIFI_BOOT_SCAN_ONLY_DIAG_FORCE_COMPARATOR_EVENT_SEQUENCE
        && FORCE_EVENT_SEQ_ARMED.load(Ordering::Relaxed)
        && (ret as u32 == 0x0000_0800 || ret == 0)
    {
        let idx = FORCE_EVENT_SEQ_NEXT.fetch_add(1, Ordering::Relaxed);
        FORCE_EVENT_SEQUENCE[idx % FORCE_EVENT_SEQ_LEN] as usize
    } else {
        ret
    };
    let post_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    let post_words = read_event_window_words();
    HAL_GET_EVENT_RING.push(Snapshot {
        arg0: 0,
        arg1: 0,
        ret: ret as u32,
        ret_forced: ret_forced as u32,
        pre_mac_isr,
        post_mac_isr,
        pre_rx_sta: pre_rx_sta as u32,
        post_rx_sta: post_rx_sta as u32,
    });
    HAL_GET_EVENT_EXT_RING.push(EventSnapshot {
        ret: ret as u32,
        ret_forced: ret_forced as u32,
        pre_mac_isr,
        post_mac_isr,
        pre_words,
        post_words,
    });
    ret_forced
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_hal_mac_interrupt_clr_event(a2: usize) -> usize {
    let pre_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { __real_hal_mac_interrupt_clr_event(a2) };
    let post_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    HAL_CLR_EVENT_RING.push(Snapshot {
        arg0: a2 as u32,
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
unsafe extern "C" fn __wrap_hal_mac_rx_get_end_info(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    let pre_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { __real_hal_mac_rx_get_end_info(a2, a3, a4, a5, a6, a7) };
    let post_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    HAL_RX_END_RING.push(Snapshot {
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
unsafe extern "C" fn __wrap_pp_post(a2: usize, a3: usize) -> usize {
    let pre_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { __real_pp_post(a2, a3) };
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
unsafe extern "C" fn __wrap_lmacProcessRxSucData() -> usize {
    let pre_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { __real_lmacProcessRxSucData() };
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
