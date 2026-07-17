use super::state::wrap_call;

unsafe extern "C" {
    static mut g_chm: u8;
    static mut g_scan: u8;
    fn __real_scan_start(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize)
        -> usize;
    fn __real_scan_pm_offchan(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_scan_start_handler(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_scan_set_scan_id(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_scan_get_scan_id(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_scan_enter_oper_channel_process(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_wifi_scan_start_process(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_scan_inter_channel_timeout_process(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_clear_bss_queue(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_ieee80211_sta_scan(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_pm_offchan(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(1, __real_scan_pm_offchan, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_start(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(2, __real_scan_start, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_start_handler(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(3, __real_scan_start_handler, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_set_scan_id(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(4, __real_scan_set_scan_id, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_get_scan_id(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(5, __real_scan_get_scan_id, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_enter_oper_channel_process(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe {
        wrap_call(
            6,
            __real_scan_enter_oper_channel_process,
            a2,
            a3,
            a4,
            a5,
            a6,
            a7,
        )
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_wifi_scan_start_process(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(7, __real_wifi_scan_start_process, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_inter_channel_timeout_process(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe {
        wrap_call(
            8,
            __real_scan_inter_channel_timeout_process,
            a2,
            a3,
            a4,
            a5,
            a6,
            a7,
        )
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_clear_bss_queue(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(9, __real_clear_bss_queue, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ieee80211_sta_scan(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    unsafe { wrap_call(10, __real_ieee80211_sta_scan, a2, a3, a4, a5, a6, a7) }
}
