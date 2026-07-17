use super::*;

pub(super) fn log_blob_state_diag(stage: &str) {
    let g_ic_ptr = core::ptr::addr_of!(g_ic) as usize;
    let g_chm_slot_ptr = core::ptr::addr_of!(g_chm) as usize;
    let g_scan_slot_ptr = core::ptr::addr_of!(g_scan) as usize;
    let g_pm_ptr = core::ptr::addr_of!(g_pm) as usize;
    let g_wifi_nvs_slot_ptr = core::ptr::addr_of!(g_wifi_nvs) as usize;
    let g_misc_nvs_slot_ptr = core::ptr::addr_of!(g_misc_nvs) as usize;
    let g_mac_sleep_en_ptr = core::ptr::addr_of!(g_mac_sleep_en) as usize;
    let connect_scan_flag_ptr = core::ptr::addr_of!(connect_scan_flag) as usize;
    let app_scan_params_ptr = core::ptr::addr_of!(app_scan_params) as usize;
    let sta_rxcb_slot_ptr = core::ptr::addr_of!(sta_rxcb) as usize;
    let ap_rxcb_slot_ptr = core::ptr::addr_of!(ap_rxcb) as usize;
    let ndp_rxcb_slot_ptr = core::ptr::addr_of!(ndp_rxcb) as usize;

    let sta_ptr = read_ptr(g_ic_ptr, 0x10);
    let ap_ptr = read_ptr(g_ic_ptr, 0x14);
    let chm_ptr = read_ptr(g_chm_slot_ptr, 0x0);
    let scan_ptr = read_ptr(g_scan_slot_ptr, 0x0);
    let wifi_nvs_ptr = read_ptr(g_wifi_nvs_slot_ptr, 0x0);

    println!(
        "upload_http: boot_scan_only_diag blob_pm after={} g_pm_ptr=0x{:x} byte_0d=0x{:02x} byte_0e=0x{:02x} byte_14=0x{:02x} word_120=0x{:08x} byte_1b8=0x{:02x} deferred_arg_slot=0x{:08x} deferred_cb_slot=0x{:08x}",
        stage,
        g_pm_ptr,
        read_u8(g_pm_ptr, 0x0d),
        read_u8(g_pm_ptr, 0x0e),
        read_u8(g_pm_ptr, 0x14),
        read_u32(g_pm_ptr, 0x120),
        read_u8(g_pm_ptr, 0x1b8),
        read_u32(g_pm_ptr.wrapping_sub(0x0c), 0x00),
        read_u32(g_pm_ptr.wrapping_sub(0x08), 0x00),
    );

    println!(
        "upload_http: boot_scan_only_diag blob_state after={} g_ic_ptr=0x{:x} sta_ptr=0x{:x} ap_ptr=0x{:x} g_chm_slot_ptr=0x{:x} chm_ptr=0x{:x} g_wifi_nvs_slot_ptr=0x{:x} g_wifi_nvs_ptr=0x{:x} wifi_nvs_byte_00=0x{:02x} wifi_nvs_byte_35c=0x{:02x} wifi_nvs_byte_361=0x{:02x} wifi_nvs_byte_364=0x{:02x} wifi_nvs_byte_415=0x{:02x} wifi_nvs_byte_417=0x{:02x} wifi_nvs_byte_418=0x{:02x} g_mac_sleep_en_ptr=0x{:x} g_mac_sleep_en={}",
        stage,
        g_ic_ptr,
        sta_ptr,
        ap_ptr,
        g_chm_slot_ptr,
        chm_ptr,
        g_wifi_nvs_slot_ptr,
        wifi_nvs_ptr,
        read_u8(wifi_nvs_ptr, 0x00),
        read_u8(wifi_nvs_ptr, 0x35c),
        read_u8(wifi_nvs_ptr, 0x361),
        read_u8(wifi_nvs_ptr, 0x364),
        read_u8(wifi_nvs_ptr, 0x415),
        read_u8(wifi_nvs_ptr, 0x417),
        read_u8(wifi_nvs_ptr, 0x418),
        g_mac_sleep_en_ptr,
        read_u8(g_mac_sleep_en_ptr, 0),
    );
    println!(
        "upload_http: boot_scan_only_diag blob_rxcb after={} sta_rxcb_slot_ptr=0x{:x} sta_rxcb_ptr=0x{:08x} ap_rxcb_slot_ptr=0x{:x} ap_rxcb_ptr=0x{:08x} ndp_rxcb_slot_ptr=0x{:x} ndp_rxcb_ptr=0x{:08x}",
        stage,
        sta_rxcb_slot_ptr,
        read_u32(sta_rxcb_slot_ptr, 0),
        ap_rxcb_slot_ptr,
        read_u32(ap_rxcb_slot_ptr, 0),
        ndp_rxcb_slot_ptr,
        read_u32(ndp_rxcb_slot_ptr, 0),
    );
    let (wifi_mac_isr_target_ptr, wifi_mac_isr_arg_ptr) =
        esp_radio::diagnostic_wifi_mac_isr_target();
    println!(
        "upload_http: boot_scan_only_diag blob_wifi_mac_isr after={} target_ptr=0x{:08x} arg_ptr=0x{:08x}",
        stage,
        wifi_mac_isr_target_ptr as u32,
        wifi_mac_isr_arg_ptr as u32,
    );
    let g_misc_nvs_target_ptr = read_u32(g_misc_nvs_slot_ptr, 0x00) as usize;
    println!(
        "upload_http: boot_scan_only_diag blob_misc_nvs after={} g_misc_nvs_slot_ptr=0x{:x} slot_word_00=0x{:08x} target_ptr=0x{:x} target_word_00=0x{:08x} target_word_04=0x{:08x} target_word_08=0x{:08x}",
        stage,
        g_misc_nvs_slot_ptr,
        read_u32(g_misc_nvs_slot_ptr, 0x00),
        g_misc_nvs_target_ptr,
        read_u32(g_misc_nvs_target_ptr, 0x00),
        read_u32(g_misc_nvs_target_ptr, 0x04),
        read_u32(g_misc_nvs_target_ptr, 0x08),
    );
    log_raw_window_diag(stage, "mac_event_window", 0x3ff73c40);
    log_raw_window_diag(stage, "mac_ctrl_window", 0x3ff73d20);
    log_raw_window_diag(stage, "mac_policy_window0", 0x3ff73020);
    log_raw_window_diag(stage, "mac_policy_window1", 0x3ff73060);

    println!(
        "upload_http: boot_scan_only_diag blob_ic after={} word_00=0x{:08x} flags_251=0x{:02x} flags_252=0x{:02x} ptr_130=0x{:x} ptr_134=0x{:x} ptr_1b4=0x{:x}",
        stage,
        read_u32(g_ic_ptr, 0x00),
        read_u8(g_ic_ptr, 0x251),
        read_u8(g_ic_ptr, 0x252),
        read_ptr(g_ic_ptr, 0x130),
        read_ptr(g_ic_ptr, 0x134),
        read_ptr(g_ic_ptr, 0x1b4),
    );
    let ic_1b4_ptr = read_ptr(g_ic_ptr, 0x1b4);
    println!(
        "upload_http: boot_scan_only_diag blob_ic_1b4_fields after={} ptr=0x{:x} bytes_34_3c={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} bytes_5d_5f={:02x}:{:02x}:{:02x} bytes_7c_8c={:02x}:{:02x}:{:02x}:{:02x}",
        stage,
        ic_1b4_ptr,
        read_u8(ic_1b4_ptr, 0x34),
        read_u8(ic_1b4_ptr, 0x35),
        read_u8(ic_1b4_ptr, 0x36),
        read_u8(ic_1b4_ptr, 0x37),
        read_u8(ic_1b4_ptr, 0x38),
        read_u8(ic_1b4_ptr, 0x39),
        read_u8(ic_1b4_ptr, 0x3a),
        read_u8(ic_1b4_ptr, 0x3b),
        read_u8(ic_1b4_ptr, 0x3c),
        read_u8(ic_1b4_ptr, 0x5d),
        read_u8(ic_1b4_ptr, 0x5e),
        read_u8(ic_1b4_ptr, 0x5f),
        read_u8(ic_1b4_ptr, 0x7c),
        read_u8(ic_1b4_ptr, 0x84),
        read_u8(ic_1b4_ptr, 0x88),
        read_u8(ic_1b4_ptr, 0x8c),
    );
    println!(
        "upload_http: boot_scan_only_diag blob_ic_parse after={} bytes_200_20a={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} bytes_29c_29f={:02x}:{:02x}:{:02x}:{:02x}",
        stage,
        read_u8(g_ic_ptr, 0x200),
        read_u8(g_ic_ptr, 0x201),
        read_u8(g_ic_ptr, 0x202),
        read_u8(g_ic_ptr, 0x203),
        read_u8(g_ic_ptr, 0x204),
        read_u8(g_ic_ptr, 0x205),
        read_u8(g_ic_ptr, 0x206),
        read_u8(g_ic_ptr, 0x207),
        read_u8(g_ic_ptr, 0x208),
        read_u8(g_ic_ptr, 0x209),
        read_u8(g_ic_ptr, 0x20a),
        read_u8(g_ic_ptr, 0x29c),
        read_u8(g_ic_ptr, 0x29d),
        read_u8(g_ic_ptr, 0x29e),
        read_u8(g_ic_ptr, 0x29f),
    );
    println!(
        "upload_http: boot_scan_only_diag blob_ic_scratch after={} ptr=0x{:x} bytes_00_0f={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        stage,
        g_ic_ptr + 0x1f0,
        read_u8(g_ic_ptr, 0x1f0),
        read_u8(g_ic_ptr, 0x1f1),
        read_u8(g_ic_ptr, 0x1f2),
        read_u8(g_ic_ptr, 0x1f3),
        read_u8(g_ic_ptr, 0x1f4),
        read_u8(g_ic_ptr, 0x1f5),
        read_u8(g_ic_ptr, 0x1f6),
        read_u8(g_ic_ptr, 0x1f7),
        read_u8(g_ic_ptr, 0x1f8),
        read_u8(g_ic_ptr, 0x1f9),
        read_u8(g_ic_ptr, 0x1fa),
        read_u8(g_ic_ptr, 0x1fb),
        read_u8(g_ic_ptr, 0x1fc),
        read_u8(g_ic_ptr, 0x1fd),
        read_u8(g_ic_ptr, 0x1fe),
        read_u8(g_ic_ptr, 0x1ff),
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
            "upload_http: boot_scan_only_diag blob_scan after={} word_00=0x{:08x} word_04=0x{:08x} word_18=0x{:08x} word_30=0x{:08x} word_34=0x{:08x} word_3c=0x{:08x} word_40=0x{:08x} byte_44=0x{:02x} byte_45=0x{:02x} byte_46=0x{:02x} byte_47=0x{:02x} byte_68=0x{:02x} byte_69=0x{:02x} flags_70=0x{:02x} flags_71=0x{:02x} byte_119=0x{:02x} word_114=0x{:08x} ptr_194=0x{:x}",
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
            read_u8(scan_ptr, 0x68),
            read_u8(scan_ptr, 0x69),
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
