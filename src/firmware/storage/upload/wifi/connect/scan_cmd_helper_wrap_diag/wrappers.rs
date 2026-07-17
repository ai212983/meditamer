use super::capture::wrap_call;

unsafe extern "C" {
    fn __real_scan_hidden_ssid(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize;
    fn __real_scan_set_current_scan_times(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize;
    fn __real_scan_build_chan_list(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize;
    fn __real_scan_set_desChan(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize;
    fn __real_ieee80211_regdomain_chan_in_range(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize;
    fn __real_ieee80211_regdomain_min_chan(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize;
    fn __real_ieee80211_regdomain_max_chan(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_hidden_ssid(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize {
    unsafe { wrap_call(1, __real_scan_hidden_ssid, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_set_current_scan_times(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize {
    unsafe { wrap_call(2, __real_scan_set_current_scan_times, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_build_chan_list(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize {
    unsafe { wrap_call(3, __real_scan_build_chan_list, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_scan_set_desChan(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize {
    unsafe { wrap_call(4, __real_scan_set_desChan, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ieee80211_regdomain_chan_in_range(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize {
    unsafe { wrap_call(5, __real_ieee80211_regdomain_chan_in_range, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ieee80211_regdomain_min_chan(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize {
    unsafe { wrap_call(6, __real_ieee80211_regdomain_min_chan, a2, a3, a4, a5, a6, a7) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ieee80211_regdomain_max_chan(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize {
    unsafe { wrap_call(7, __real_ieee80211_regdomain_max_chan, a2, a3, a4, a5, a6, a7) }
}
