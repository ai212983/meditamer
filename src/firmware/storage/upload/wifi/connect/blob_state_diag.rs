use esp_println::println;
use esp_wifi_sys::c_types::c_void;

unsafe extern "C" {
    static mut g_ic: u8;
    static mut g_cnxMgr: u8;
    static mut g_chm: u8;
    static mut g_scan: u8;
    static mut g_pm: u8;
    static mut g_wifi_nvs: u8;
    static mut g_misc_nvs: u8;
    static mut g_rssi_threshold_failure: u8;
    static mut g_authmode_threshold_failure: u8;
    static mut g_authmode_incompatible: u8;
    static mut g_in_blacklist_flag: u8;
    static mut g_mac_sleep_en: u8;
    static mut connect_scan_flag: u8;
    static mut app_scan_params: u8;
    static mut scannum: u8;
    static mut sta_rxcb: u8;
    static mut ap_rxcb: u8;
    static mut ndp_rxcb: u8;
}

fn read_u8(base: usize, offset: usize) -> u8 {
    if base == 0 {
        return 0;
    }
    unsafe { ((base + offset) as *const u8).read_volatile() }
}

fn read_u16(base: usize, offset: usize) -> u16 {
    if base == 0 {
        return 0;
    }
    unsafe { ((base + offset) as *const u16).read_volatile() }
}

fn read_u32(base: usize, offset: usize) -> u32 {
    if base == 0 {
        return 0;
    }
    unsafe { ((base + offset) as *const u32).read_volatile() }
}

fn read_ptr(base: usize, offset: usize) -> usize {
    read_u32(base, offset) as usize
}

fn log_raw_window_diag(stage: &str, label: &str, base: usize) {
    println!(
        "upload_http: boot_scan_only_diag blob_hal after={} label={} base=0x{:x} w0=0x{:08x} w1=0x{:08x} w2=0x{:08x} w3=0x{:08x} w4=0x{:08x} w5=0x{:08x}",
        stage,
        label,
        base,
        read_u32(base, 0x00),
        read_u32(base, 0x04),
        read_u32(base, 0x08),
        read_u32(base, 0x0c),
        read_u32(base, 0x10),
        read_u32(base, 0x14),
    );
}

fn log_cnx_mgr_slots_diag(stage: &str) {
    let cnx_mgr_ptr = core::ptr::addr_of!(g_cnxMgr) as usize;
    const SLOT_STRIDE: usize = 0x3b8;
    const SLOT_BASE: usize = 0x08;
    for idx in 0..4usize {
        let slot_ptr = cnx_mgr_ptr + SLOT_BASE + (idx * SLOT_STRIDE);
        println!(
            "upload_http: boot_scan_only_diag blob_cnx_slot after={} idx={} slot_ptr=0x{:x} bssid={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} word_0c=0x{:08x} word_2a8=0x{:08x} word_2ac=0x{:08x}",
            stage,
            idx,
            slot_ptr,
            read_u8(slot_ptr, 0x04),
            read_u8(slot_ptr, 0x05),
            read_u8(slot_ptr, 0x06),
            read_u8(slot_ptr, 0x07),
            read_u8(slot_ptr, 0x08),
            read_u8(slot_ptr, 0x09),
            read_u32(slot_ptr, 0x0c),
            read_u32(slot_ptr, 0x2a8),
            read_u32(slot_ptr, 0x2ac),
        );
    }
}

fn log_scan_history_diag(stage: &str, scan_ptr: usize) {
    let history_count = read_u8(scan_ptr, 0x110);
    println!(
        "upload_http: boot_scan_only_diag blob_scan_history after={} count=0x{:02x}",
        stage, history_count,
    );
    for idx in 0..3usize {
        let row_ptr = scan_ptr + 0x0a4 + (idx * 0x20);
        println!(
            "upload_http: boot_scan_only_diag blob_scan_history_row after={} idx={} row_ptr=0x{:x} bytes_00_07={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} meta_c5=0x{:02x} meta_c6=0x{:02x} meta_c7=0x{:02x}",
            stage,
            idx,
            row_ptr,
            read_u8(row_ptr, 0x00),
            read_u8(row_ptr, 0x01),
            read_u8(row_ptr, 0x02),
            read_u8(row_ptr, 0x03),
            read_u8(row_ptr, 0x04),
            read_u8(row_ptr, 0x05),
            read_u8(row_ptr, 0x06),
            read_u8(row_ptr, 0x07),
            read_u8(row_ptr, 0x21),
            read_u8(row_ptr, 0x22),
            read_u8(row_ptr, 0x23),
        );
    }
}

fn count_scan_history_nonzero_rows(scan_ptr: usize) -> u32 {
    let mut count = 0u32;
    for idx in 0..3usize {
        let row_ptr = scan_ptr + 0x0a4 + (idx * 0x20);
        let row_nonzero = (0..8usize).any(|offset| read_u8(row_ptr, offset) != 0)
            || read_u8(row_ptr, 0x21) != 0
            || read_u8(row_ptr, 0x22) != 0
            || read_u8(row_ptr, 0x23) != 0;
        if row_nonzero {
            count += 1;
        }
    }
    count
}

fn count_cnx_mgr_nonzero_slots() -> u32 {
    let cnx_mgr_ptr = core::ptr::addr_of!(g_cnxMgr) as usize;
    const SLOT_STRIDE: usize = 0x3b8;
    const SLOT_BASE: usize = 0x08;
    let mut count = 0u32;
    for idx in 0..4usize {
        let slot_ptr = cnx_mgr_ptr + SLOT_BASE + (idx * SLOT_STRIDE);
        let slot_nonzero = (0..6usize).any(|offset| read_u8(slot_ptr, 0x04 + offset) != 0)
            || read_u32(slot_ptr, 0x0c) != 0
            || read_u32(slot_ptr, 0x2a8) != 0
            || read_u32(slot_ptr, 0x2ac) != 0;
        if slot_nonzero {
            count += 1;
        }
    }
    count
}

fn count_cnx_mgr_seeded_slots() -> u32 {
    let cnx_mgr_ptr = core::ptr::addr_of!(g_cnxMgr) as usize;
    const SLOT_STRIDE: usize = 0x3b8;
    const SLOT_BASE: usize = 0x08;
    let mut count = 0u32;
    for idx in 0..4usize {
        let slot_ptr = cnx_mgr_ptr + SLOT_BASE + (idx * SLOT_STRIDE);
        if read_u32(slot_ptr, 0x04) != 0 || read_u16(slot_ptr, 0x08) != 0 {
            count += 1;
        }
    }
    count
}

mod scan;
mod state;

pub(super) use scan::{
    log_scan_done_failure_blob_diag, log_scan_done_list_diag, log_scan_list_probe_diag,
    log_scan_prelink_summary_diag,
};
pub(super) use state::log_blob_state_diag;
#[allow(dead_code)]
fn _type_anchor(_: *const c_void) {}
