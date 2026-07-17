use super::*;
use super::rx::WDEV_PROCESS_RX_DIRECT_RING;
use super::trampolines::{
    lmac_process_rx_suc_data_trampoline, pp_post_trampoline,
    wdev_process_panic_watchdog_trampoline, wdev_process_rx_suc_data_trampoline,
};
use esp_println::println;

pub(super) fn reset() {
    keep_wdev_branch_trampolines();
    HAL_RX_END_RING.reset();
    HAL_GET_EVENT_RING.reset();
    HAL_CLR_EVENT_RING.reset();
    HAL_GET_EVENT_EXT_RING.reset();
    PANIC_WATCHDOG_RING.reset();
    PP_POST_RING.reset();
    LMAC_RX_SUC_RING.reset();
    WDEV_PROCESS_RX_DIRECT_RING.reset();
    PP_POST_ARG25_COUNT.store(0, Ordering::Relaxed);
    FORCE_EVENT_SEQ_NEXT.store(0, Ordering::Relaxed);
    FORCE_EVENT_SEQ_ARMED.store(false, Ordering::Relaxed);
}

pub(super) fn set_force_comparator_event_sequence_diag_armed(armed: bool) {
    FORCE_EVENT_SEQ_ARMED.store(armed, Ordering::Relaxed);
    if !armed {
        FORCE_EVENT_SEQ_NEXT.store(0, Ordering::Relaxed);
    }
}

pub(super) fn log(stage: &str) {
    HAL_GET_EVENT_RING.log(stage, "hal_mac_get_event_wrap_diag");
    HAL_CLR_EVENT_RING.log(stage, "hal_mac_clr_event_wrap_diag");
    HAL_GET_EVENT_EXT_RING.log(stage, "hal_mac_get_event_wrap_diag_ext");
    log_mac_event_window_snapshot(stage);
    PANIC_WATCHDOG_RING.log(stage, "wdev_panic_watchdog_wrap_diag");
    HAL_RX_END_RING.log(stage, "hal_mac_rx_end_wrap_diag");
    PP_POST_RING.log(stage, "pp_post_wrap_diag");
    LMAC_RX_SUC_RING.log(stage, "lmac_rx_suc_wrap_diag");
    WDEV_PROCESS_RX_DIRECT_RING.log(stage);
}

#[inline(never)]
#[unsafe(no_mangle)]
pub extern "C" fn keep_wdev_branch_trampolines() {
    let func_watchdog = wdev_process_panic_watchdog_trampoline as usize;
    let func_lmac_rx = lmac_process_rx_suc_data_trampoline as usize;
    let func_pp_post = pp_post_trampoline as usize;
    let func_wdev_rx = wdev_process_rx_suc_data_trampoline as usize;
    unsafe {
        core::ptr::read_volatile(&func_watchdog);
        core::ptr::read_volatile(&func_lmac_rx);
        core::ptr::read_volatile(&func_pp_post);
        core::ptr::read_volatile(&func_wdev_rx);
    }
}

pub(super) fn read_event_window_words() -> [u32; EVENT_WORDS] {
    read_event_window_words_offset(0)
}

fn log_mac_event_window_snapshot(stage: &str) {
    let words0 = read_event_window_words_offset(0);
    let words1 = read_event_window_words_offset(EVENT_WORDS);
    println!(
        "upload_http: boot_scan_only_diag mac_event_window after={} words0_5={:08x}:{:08x}:{:08x}:{:08x}:{:08x}:{:08x} words6_11={:08x}:{:08x}:{:08x}:{:08x}:{:08x}:{:08x}",
        stage,
        words0[0],
        words0[1],
        words0[2],
        words0[3],
        words0[4],
        words0[5],
        words1[0],
        words1[1],
        words1[2],
        words1[3],
        words1[4],
        words1[5],
    );
}

fn read_event_window_words_offset(offset_words: usize) -> [u32; EVENT_WORDS] {
    let mut out = [0u32; EVENT_WORDS];
    let mut idx = 0usize;
    while idx < EVENT_WORDS {
        out[idx] = unsafe {
            ((MAC_EVENT_WINDOW_BASE + (offset_words + idx) * 4) as *const u32).read_volatile()
        };
        idx += 1;
    }
    out
}

pub(super) fn log_binary_patch_counts(stage: &str) {
    println!(
        "upload_http: boot_scan_only_diag wdev_binary_patch_counts after={} watchdog_count={} lmac_rx_suc_count={} pp_post_arg25_count={}",
        stage,
        PANIC_WATCHDOG_RING.next.load(Ordering::Relaxed).min(SLOT_COUNT),
        LMAC_RX_SUC_RING.next.load(Ordering::Relaxed).min(SLOT_COUNT),
        PP_POST_ARG25_COUNT.load(Ordering::Relaxed),
    );
}
