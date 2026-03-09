use esp_println::println;
use esp_wifi_sys::c_types::c_void;

unsafe extern "C" {
    static mut g_ic: u8;
    static mut g_cnxMgr: u8;
    static mut g_chm: u8;
    static mut g_scan: u8;
    static mut g_wifi_nvs: u8;
    static mut g_mac_sleep_en: u8;
    static mut connect_scan_flag: u8;
    static mut app_scan_params: u8;
    static mut scannum: u8;
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

pub(super) fn log_blob_state_diag(stage: &str) {
    let g_ic_ptr = core::ptr::addr_of!(g_ic) as usize;
    let g_chm_slot_ptr = core::ptr::addr_of!(g_chm) as usize;
    let g_scan_slot_ptr = core::ptr::addr_of!(g_scan) as usize;
    let g_wifi_nvs_ptr = core::ptr::addr_of!(g_wifi_nvs) as usize;
    let g_mac_sleep_en_ptr = core::ptr::addr_of!(g_mac_sleep_en) as usize;
    let connect_scan_flag_ptr = core::ptr::addr_of!(connect_scan_flag) as usize;
    let app_scan_params_ptr = core::ptr::addr_of!(app_scan_params) as usize;

    let sta_ptr = read_ptr(g_ic_ptr, 0x10);
    let ap_ptr = read_ptr(g_ic_ptr, 0x14);
    let chm_ptr = read_ptr(g_chm_slot_ptr, 0x0);
    let scan_ptr = read_ptr(g_scan_slot_ptr, 0x0);
    let wifi_mode = read_u8(g_wifi_nvs_ptr, 0x0);

    println!(
        "upload_http: boot_scan_only_diag blob_state after={} g_ic_ptr=0x{:x} sta_ptr=0x{:x} ap_ptr=0x{:x} g_chm_slot_ptr=0x{:x} chm_ptr=0x{:x} g_wifi_nvs_ptr=0x{:x} wifi_mode={} wifi_nvs_byte_35c=0x{:02x} g_mac_sleep_en_ptr=0x{:x} g_mac_sleep_en={}",
        stage,
        g_ic_ptr,
        sta_ptr,
        ap_ptr,
        g_chm_slot_ptr,
        chm_ptr,
        g_wifi_nvs_ptr,
        wifi_mode,
        read_u8(g_wifi_nvs_ptr, 0x35c),
        g_mac_sleep_en_ptr,
        read_u8(g_mac_sleep_en_ptr, 0),
    );

    println!(
        "upload_http: boot_scan_only_diag blob_ic after={} flags_251=0x{:02x} flags_252=0x{:02x} ptr_130=0x{:x} ptr_134=0x{:x} ptr_1b4=0x{:x}",
        stage,
        read_u8(g_ic_ptr, 0x251),
        read_u8(g_ic_ptr, 0x252),
        read_ptr(g_ic_ptr, 0x130),
        read_ptr(g_ic_ptr, 0x134),
        read_ptr(g_ic_ptr, 0x1b4),
    );

    if sta_ptr != 0 {
        println!(
            "upload_http: boot_scan_only_diag blob_sta after={} flag_154=0x{:02x} flag_155=0x{:02x} val_15e=0x{:04x} word_0e4=0x{:08x}",
            stage,
            read_u8(sta_ptr, 0x154),
            read_u8(sta_ptr, 0x155),
            read_u16(sta_ptr, 0x15e),
            read_u32(sta_ptr, 0x0e4),
        );
    }

    if chm_ptr != 0 {
        println!(
            "upload_http: boot_scan_only_diag blob_chm after={} op_chan=0x{:02x} op_mode=0x{:02x} ptr_08=0x{:x} ptr_0c=0x{:x} ptr_10=0x{:x} ptr_14=0x{:x} home_chan=0x{:02x} home_mode=0x{:02x} current_chan=0x{:02x} current_mode=0x{:02x}",
            stage,
            read_u8(chm_ptr, 0x04),
            read_u8(chm_ptr, 0x05),
            read_ptr(chm_ptr, 0x08),
            read_ptr(chm_ptr, 0x0c),
            read_ptr(chm_ptr, 0x10),
            read_ptr(chm_ptr, 0x14),
            read_u8(chm_ptr, 0x50),
            read_u8(chm_ptr, 0x51),
            read_u8(chm_ptr, 0x52),
            read_u8(chm_ptr, 0x53),
        );
    }

    println!(
        "upload_http: boot_scan_only_diag blob_scan_globals after={} g_scan_slot_ptr=0x{:x} scan_ptr=0x{:x} connect_scan_flag_ptr=0x{:x} connect_scan_flag=0x{:02x} app_scan_params_ptr=0x{:x} app_scan_params_word0=0x{:08x} app_scan_params_word1=0x{:08x} app_scan_params_word2=0x{:08x} app_scan_params_word3=0x{:08x}",
        stage,
        g_scan_slot_ptr,
        scan_ptr,
        connect_scan_flag_ptr,
        read_u8(connect_scan_flag_ptr, 0),
        app_scan_params_ptr,
        read_u32(app_scan_params_ptr, 0x00),
        read_u32(app_scan_params_ptr, 0x04),
        read_u32(app_scan_params_ptr, 0x08),
        read_u32(app_scan_params_ptr, 0x0c),
    );

    if scan_ptr != 0 {
        println!(
            "upload_http: boot_scan_only_diag blob_scan after={} word_00=0x{:08x} word_04=0x{:08x} word_18=0x{:08x} word_30=0x{:08x} word_34=0x{:08x} word_3c=0x{:08x} word_40=0x{:08x} byte_44=0x{:02x} byte_45=0x{:02x} byte_46=0x{:02x} byte_47=0x{:02x} flags_70=0x{:02x} flags_71=0x{:02x} byte_119=0x{:02x} word_114=0x{:08x} ptr_194=0x{:x}",
            stage,
            read_u32(scan_ptr, 0x00),
            read_u32(scan_ptr, 0x04),
            read_u32(scan_ptr, 0x18),
            read_u32(scan_ptr, 0x30),
            read_u32(scan_ptr, 0x34),
            read_u32(scan_ptr, 0x3c),
            read_u32(scan_ptr, 0x40),
            read_u8(scan_ptr, 0x44),
            read_u8(scan_ptr, 0x45),
            read_u8(scan_ptr, 0x46),
            read_u8(scan_ptr, 0x47),
            read_u8(scan_ptr, 0x46),
            read_u8(scan_ptr, 0x47),
            read_u8(scan_ptr, 0x119),
            read_u32(scan_ptr, 0x114),
            read_ptr(scan_ptr, 0x194),
        );
        log_scan_history_diag(stage, scan_ptr);
    }

    log_cnx_mgr_slots_diag(stage);
}

pub(super) fn log_scan_done_list_diag(status: u32, count: u32, scan_id: u32) {
    let g_ic_ptr = core::ptr::addr_of!(g_ic) as usize;
    let scannum_ptr = core::ptr::addr_of!(scannum) as usize;
    let head_ptr = read_ptr(g_ic_ptr, 0x130);
    let tail_ptr = read_ptr(g_ic_ptr, 0x134);
    println!(
        "upload_http: event scan_done_list status={} count={} scan_id={} scannum=0x{:04x} head_ptr=0x{:x} tail_ptr=0x{:x}",
        status,
        count,
        scan_id,
        read_u16(scannum_ptr, 0),
        head_ptr,
        tail_ptr,
    );
    if head_ptr != 0 {
        println!(
            "upload_http: event scan_done_list_head head_ptr=0x{:x} word_00=0x{:08x} word_04=0x{:08x} word_08=0x{:08x} word_0c=0x{:08x}",
            head_ptr,
            read_u32(head_ptr, 0x00),
            read_u32(head_ptr, 0x04),
            read_u32(head_ptr, 0x08),
            read_u32(head_ptr, 0x0c),
        );
    }
}

#[allow(dead_code)]
fn _type_anchor(_: *const c_void) {}
