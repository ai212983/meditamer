use esp_println::println;

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

fn read_u32(base: usize, offset: usize) -> u32 {
    if base == 0 {
        return 0;
    }
    unsafe { ((base + offset) as *const u32).read_unaligned() }
}

fn read_ptr(base: usize, offset: usize) -> usize {
    read_u32(base, offset) as usize
}

fn print_raw_window(label: &str, window_label: &str, base: usize) {
    println!(
        "legacy_nostd_wifi_control: blob_hal label={} window={} base=0x{:08x} w0=0x{:08x} w1=0x{:08x} w2=0x{:08x} w3=0x{:08x} w4=0x{:08x} w5=0x{:08x}",
        label,
        window_label,
        base,
        read_u32(base, 0x00),
        read_u32(base, 0x04),
        read_u32(base, 0x08),
        read_u32(base, 0x0c),
        read_u32(base, 0x10),
        read_u32(base, 0x14),
    );
}

pub(crate) fn print_blob_state(label: &str) {
    let g_ic_ptr = core::ptr::addr_of!(g_ic) as usize;
    let g_cnx_mgr_ptr = core::ptr::addr_of!(g_cnxMgr) as usize;
    let g_chm_slot_ptr = core::ptr::addr_of!(g_chm) as usize;
    let g_scan_slot_ptr = core::ptr::addr_of!(g_scan) as usize;
    let g_wifi_nvs_slot_ptr = core::ptr::addr_of!(g_wifi_nvs) as usize;
    let g_mac_sleep_en_ptr = core::ptr::addr_of!(g_mac_sleep_en) as usize;
    let connect_scan_flag_ptr = core::ptr::addr_of!(connect_scan_flag) as usize;
    let app_scan_params_ptr = core::ptr::addr_of!(app_scan_params) as usize;
    let sta_ptr = read_ptr(g_ic_ptr, 0x10);
    let ap_ptr = read_ptr(g_ic_ptr, 0x14);
    let chm_ptr = read_ptr(g_chm_slot_ptr, 0x0);
    let scan_ptr = read_ptr(g_scan_slot_ptr, 0x0);
    let wifi_nvs_ptr = read_ptr(g_wifi_nvs_slot_ptr, 0x0);

    println!(
        "legacy_nostd_wifi_control: blob_state label={} g_ic_ptr=0x{:08x} sta_ptr=0x{:08x} ap_ptr=0x{:08x} g_chm_slot_ptr=0x{:08x} chm_ptr=0x{:08x} g_wifi_nvs_slot_ptr=0x{:08x} g_wifi_nvs_ptr=0x{:08x} wifi_nvs_byte_00=0x{:02x} wifi_nvs_byte_35c=0x{:02x} g_mac_sleep_en_ptr=0x{:08x} g_mac_sleep_en={}",
        label,
        g_ic_ptr,
        sta_ptr,
        ap_ptr,
        g_chm_slot_ptr,
        chm_ptr,
        g_wifi_nvs_slot_ptr,
        wifi_nvs_ptr,
        read_u8(wifi_nvs_ptr, 0x00),
        read_u8(wifi_nvs_ptr, 0x35c),
        g_mac_sleep_en_ptr,
        read_u8(g_mac_sleep_en_ptr, 0),
    );
    println!(
        "legacy_nostd_wifi_control: blob_ic label={} word_00=0x{:08x} flags_251=0x{:02x} flags_252=0x{:02x} ptr_130=0x{:08x} ptr_134=0x{:08x} ptr_1b4=0x{:08x}",
        label,
        read_u32(g_ic_ptr, 0x00),
        read_u8(g_ic_ptr, 0x251),
        read_u8(g_ic_ptr, 0x252),
        read_ptr(g_ic_ptr, 0x130),
        read_ptr(g_ic_ptr, 0x134),
        read_ptr(g_ic_ptr, 0x1b4),
    );
    if sta_ptr != 0 {
        println!(
            "legacy_nostd_wifi_control: blob_sta label={} flag_154=0x{:02x} flag_155=0x{:02x} val_15e=0x{:04x} word_0e4=0x{:08x}",
            label,
            read_u8(sta_ptr, 0x154),
            read_u8(sta_ptr, 0x155),
            read_u32(sta_ptr, 0x15e) & 0xffff,
            read_u32(sta_ptr, 0x0e4),
        );
    }
    println!(
        "legacy_nostd_wifi_control: blob_chm label={} chm_ptr=0x{:08x} op_chan=0x{:02x} op_mode=0x{:02x} ptr_08=0x{:08x} ptr_0c=0x{:08x} ptr_10=0x{:08x} ptr_14=0x{:08x} home_chan=0x{:02x} home_mode=0x{:02x} current_chan=0x{:02x} current_mode=0x{:02x}",
        label,
        chm_ptr,
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
    println!(
        "legacy_nostd_wifi_control: blob_scan_globals label={} g_scan_slot_ptr=0x{:08x} scan_ptr=0x{:08x} connect_scan_flag_ptr=0x{:08x} connect_scan_flag=0x{:02x} app_scan_params_ptr=0x{:08x} app_scan_params_word0=0x{:08x} app_scan_params_word1=0x{:08x} app_scan_params_word2=0x{:08x} app_scan_params_word3=0x{:08x}",
        label,
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
    println!(
        "legacy_nostd_wifi_control: blob_scan label={} scan_ptr=0x{:08x} word_00=0x{:08x} word_04=0x{:08x} word_18=0x{:08x} word_30=0x{:08x} word_34=0x{:08x} word_3c=0x{:08x} word_40=0x{:08x} byte_44=0x{:02x} byte_45=0x{:02x} byte_46=0x{:02x} byte_47=0x{:02x} byte_68=0x{:02x} byte_69=0x{:02x} flags_70=0x{:02x} flags_71=0x{:02x} word_114=0x{:08x} scannum=0x{:04x}",
        label,
        scan_ptr,
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
        read_u8(scan_ptr, 0x68),
        read_u8(scan_ptr, 0x69),
        read_u8(scan_ptr, 0x70),
        read_u8(scan_ptr, 0x71),
        read_u32(scan_ptr, 0x114),
        read_u32(core::ptr::addr_of!(scannum) as usize, 0x0) & 0xffff,
    );
    print_raw_window(label, "mac_event_window", 0x3ff73c40);
    print_raw_window(label, "mac_ctrl_window", 0x3ff73d20);
    print_raw_window(label, "mac_policy_window0", 0x3ff73020);
    print_raw_window(label, "mac_policy_window1", 0x3ff73060);
    for idx in 0..3usize {
        let row_ptr = scan_ptr + 0x0a4 + (idx * 0x20);
        println!(
            "legacy_nostd_wifi_control: blob_scan_history_row label={} idx={} row_ptr=0x{:08x} bytes_00_07={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            label,
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
        );
    }
    for idx in 0..4usize {
        let slot_ptr = g_cnx_mgr_ptr + 0x08 + (idx * 0x3b8);
        println!(
            "legacy_nostd_wifi_control: blob_cnx_slot label={} idx={} slot_ptr=0x{:08x} bssid={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} word_0c=0x{:08x} word_2a8=0x{:08x} word_2ac=0x{:08x}",
            label,
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
