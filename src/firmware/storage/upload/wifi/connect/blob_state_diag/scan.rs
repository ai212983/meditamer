use super::*;

pub(super) fn log_scan_done_list_diag(status: u32, count: u32, scan_id: u32) {
    let g_ic_ptr = core::ptr::addr_of!(g_ic) as usize;
    let scannum_ptr = core::ptr::addr_of!(scannum) as usize;
    let head_ptr = read_ptr(g_ic_ptr, 0x130);
    let tail_ptr = read_ptr(g_ic_ptr, 0x134);
    println!(
        "upload_http: event scan_done_list status={} count={} scan_id={} scannum=0x{:04x} head_ptr=0x{:x} tail_ptr=0x{:x} fail_rssi={} fail_auth_threshold={} fail_auth_incompat={} fail_blacklist={}",
        status,
        count,
        scan_id,
        read_u16(scannum_ptr, 0),
        head_ptr,
        tail_ptr,
        read_u8(core::ptr::addr_of!(g_rssi_threshold_failure) as usize, 0),
        read_u8(core::ptr::addr_of!(g_authmode_threshold_failure) as usize, 0),
        read_u8(core::ptr::addr_of!(g_authmode_incompatible) as usize, 0),
        read_u8(core::ptr::addr_of!(g_in_blacklist_flag) as usize, 0),
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
    if tail_ptr != 0 {
        println!(
            "upload_http: event scan_done_list_tail tail_ptr=0x{:x} word_00=0x{:08x} word_04=0x{:08x} word_08=0x{:08x} word_0c=0x{:08x}",
            tail_ptr,
            read_u32(tail_ptr, 0x00),
            read_u32(tail_ptr, 0x04),
            read_u32(tail_ptr, 0x08),
            read_u32(tail_ptr, 0x0c),
        );
    }
}

pub(super) fn log_scan_list_probe_diag(label: &str, phase: &str) {
    let g_ic_ptr = core::ptr::addr_of!(g_ic) as usize;
    let scannum_ptr = core::ptr::addr_of!(scannum) as usize;
    let head_ptr = read_ptr(g_ic_ptr, 0x130);
    let tail_ptr = read_ptr(g_ic_ptr, 0x134);
    println!(
        "upload_http: boot_scan_only_diag scan_list_probe label={} phase={} scannum=0x{:04x} head_ptr=0x{:x} tail_ptr=0x{:x} ic_ptr_1b4=0x{:x} fail_rssi={} fail_auth_threshold={} fail_auth_incompat={} fail_blacklist={}",
        label,
        phase,
        read_u16(scannum_ptr, 0),
        head_ptr,
        tail_ptr,
        read_ptr(g_ic_ptr, 0x1b4),
        read_u8(core::ptr::addr_of!(g_rssi_threshold_failure) as usize, 0),
        read_u8(core::ptr::addr_of!(g_authmode_threshold_failure) as usize, 0),
        read_u8(core::ptr::addr_of!(g_authmode_incompatible) as usize, 0),
        read_u8(core::ptr::addr_of!(g_in_blacklist_flag) as usize, 0),
    );
    if head_ptr != 0 {
        println!(
            "upload_http: boot_scan_only_diag scan_list_probe_head label={} phase={} ptr=0x{:x} word_00=0x{:08x} word_04=0x{:08x} word_08=0x{:08x} word_0c=0x{:08x}",
            label,
            phase,
            head_ptr,
            read_u32(head_ptr, 0x00),
            read_u32(head_ptr, 0x04),
            read_u32(head_ptr, 0x08),
            read_u32(head_ptr, 0x0c),
        );
    }
    if tail_ptr != 0 {
        println!(
            "upload_http: boot_scan_only_diag scan_list_probe_tail label={} phase={} ptr=0x{:x} word_00=0x{:08x} word_04=0x{:08x} word_08=0x{:08x} word_0c=0x{:08x}",
            label,
            phase,
            tail_ptr,
            read_u32(tail_ptr, 0x00),
            read_u32(tail_ptr, 0x04),
            read_u32(tail_ptr, 0x08),
            read_u32(tail_ptr, 0x0c),
        );
    }
}

pub(super) fn log_scan_prelink_summary_diag(label: &str, phase: &str) {
    let g_scan_slot_ptr = core::ptr::addr_of!(g_scan) as usize;
    let scan_ptr = read_ptr(g_scan_slot_ptr, 0x0);
    let g_ic_ptr = core::ptr::addr_of!(g_ic) as usize;
    let g_wifi_nvs_slot_ptr = core::ptr::addr_of!(g_wifi_nvs) as usize;
    let wifi_nvs_ptr = read_ptr(g_wifi_nvs_slot_ptr, 0x0);
    let g_misc_nvs_slot_ptr = core::ptr::addr_of!(g_misc_nvs) as usize;
    let g_misc_nvs_target_ptr = read_u32(g_misc_nvs_slot_ptr, 0x00) as usize;
    let history_count = read_u8(scan_ptr, 0x110);
    let history_nonzero_rows = count_scan_history_nonzero_rows(scan_ptr);
    let cnx_nonzero_slots = count_cnx_mgr_nonzero_slots();
    let cnx_seeded_slots = count_cnx_mgr_seeded_slots();
    let adapter_diag = esp_radio::diagnostic_wifi_adapter_primitive_diag();
    let (rx_sta, rx_ap) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    println!(
        "upload_http: boot_scan_only_diag scan_prelink_summary label={} phase={} rx_sta={} rx_ap={} history_count=0x{:02x} history_nonzero_rows={} cnx_nonzero_slots={} cnx_seeded_slots={} malloc_internal_count={} wifi_malloc_count={} wifi_calloc_count={} free_count={} fail_rssi={} fail_auth_threshold={} fail_auth_incompat={} fail_blacklist={} ic_byte_18=0x{:02x} ic_byte_29f=0x{:02x} ic_byte_2a5=0x{:02x} wifi_nvs_byte_361=0x{:02x} wifi_nvs_byte_415=0x{:02x} misc_nvs_slot_word_00=0x{:08x} misc_nvs_target_ptr=0x{:x} misc_nvs_target_word_00=0x{:08x} misc_nvs_target_word_04=0x{:08x} misc_nvs_target_word_08=0x{:08x}",
        label,
        phase,
        rx_sta,
        rx_ap,
        history_count,
        history_nonzero_rows,
        cnx_nonzero_slots,
        cnx_seeded_slots,
        adapter_diag.malloc_internal_count,
        adapter_diag.wifi_malloc_count,
        adapter_diag.wifi_calloc_count,
        adapter_diag.free_count,
        read_u8(core::ptr::addr_of!(g_rssi_threshold_failure) as usize, 0),
        read_u8(core::ptr::addr_of!(g_authmode_threshold_failure) as usize, 0),
        read_u8(core::ptr::addr_of!(g_authmode_incompatible) as usize, 0),
        read_u8(core::ptr::addr_of!(g_in_blacklist_flag) as usize, 0),
        read_u8(g_ic_ptr, 0x18),
        read_u8(g_ic_ptr, 0x29f),
        read_u8(g_ic_ptr, 0x2a5),
        read_u8(wifi_nvs_ptr, 0x361),
        read_u8(wifi_nvs_ptr, 0x415),
        read_u32(g_misc_nvs_slot_ptr, 0x00),
        g_misc_nvs_target_ptr,
        read_u32(g_misc_nvs_target_ptr, 0x00),
        read_u32(g_misc_nvs_target_ptr, 0x04),
        read_u32(g_misc_nvs_target_ptr, 0x08),
    );
}

pub(super) fn log_scan_done_failure_blob_diag(status: u32, count: u32, scan_id: u32) {
    let stage = if status == 0 {
        "scan_done_ok"
    } else {
        "scan_done_fail"
    };
    println!(
        "upload_http: event scan_done_blob_diag status={} count={} scan_id={} stage={}",
        status, count, scan_id, stage
    );
    log_blob_state_diag(stage);
}
