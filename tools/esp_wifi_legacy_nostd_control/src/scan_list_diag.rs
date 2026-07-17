use esp_println::println;

unsafe extern "C" {
    static mut g_ic: u8;
    static mut g_cnxMgr: u8;
    static mut g_scan: u8;
    static mut g_wifi_nvs: u8;
    static mut g_misc_nvs: u8;
    static mut g_rssi_threshold_failure: u8;
    static mut g_authmode_threshold_failure: u8;
    static mut g_authmode_incompatible: u8;
    static mut g_in_blacklist_flag: u8;
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

fn count_scan_history_nonzero_rows(scan_ptr: usize) -> u32 {
    let mut count = 0u32;
    for idx in 0..3usize {
        let row_ptr = scan_ptr + 0x0a4 + (idx * 0x20);
        let row_nonzero = (0..8usize).any(|offset| read_u8(row_ptr, offset) != 0);
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

pub(crate) fn print_scan_list_probe(label: &str, phase: &str) {
    let g_ic_ptr = core::ptr::addr_of!(g_ic) as usize;
    let scannum_ptr = core::ptr::addr_of!(scannum) as usize;
    let head_ptr = read_ptr(g_ic_ptr, 0x130);
    let tail_ptr = read_ptr(g_ic_ptr, 0x134);
    println!(
        "legacy_nostd_wifi_control: scan_list_probe label={} phase={} scannum=0x{:04x} head_ptr=0x{:08x} tail_ptr=0x{:08x} ic_ptr_1b4=0x{:08x} fail_rssi={} fail_auth_threshold={} fail_auth_incompat={} fail_blacklist={}",
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
            "legacy_nostd_wifi_control: scan_list_probe_head label={} phase={} ptr=0x{:08x} word_00=0x{:08x} word_04=0x{:08x} word_08=0x{:08x} word_0c=0x{:08x}",
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
            "legacy_nostd_wifi_control: scan_list_probe_tail label={} phase={} ptr=0x{:08x} word_00=0x{:08x} word_04=0x{:08x} word_08=0x{:08x} word_0c=0x{:08x}",
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

pub(crate) fn print_scan_prelink_summary(label: &str, phase: &str) {
    let g_ic_ptr = core::ptr::addr_of!(g_ic) as usize;
    let scan_slot_ptr = core::ptr::addr_of!(g_scan) as usize;
    let scan_ptr = read_ptr(scan_slot_ptr, 0x0);
    let wifi_nvs_slot_ptr = core::ptr::addr_of!(g_wifi_nvs) as usize;
    let wifi_nvs_ptr = read_ptr(wifi_nvs_slot_ptr, 0x0);
    let g_misc_nvs_slot_ptr = core::ptr::addr_of!(g_misc_nvs) as usize;
    let g_misc_nvs_target_ptr = read_u32(g_misc_nvs_slot_ptr, 0x00) as usize;
    let (rx_sta, rx_ap) = esp_wifi::wifi::diagnostic_wifi_rx_cb_counts();
    println!(
        "legacy_nostd_wifi_control: scan_prelink_summary label={} phase={} rx_sta={} rx_ap={} history_count=0x{:02x} history_nonzero_rows={} cnx_nonzero_slots={} cnx_seeded_slots={} fail_rssi={} fail_auth_threshold={} fail_auth_incompat={} fail_blacklist={} ic_byte_18=0x{:02x} ic_byte_29f=0x{:02x} ic_byte_2a5=0x{:02x} wifi_nvs_byte_361=0x{:02x} wifi_nvs_byte_364=0x{:02x} wifi_nvs_byte_415=0x{:02x} wifi_nvs_byte_417=0x{:02x} wifi_nvs_byte_418=0x{:02x} misc_nvs_slot_word_00=0x{:08x} misc_nvs_target_ptr=0x{:x} misc_nvs_target_word_00=0x{:08x} misc_nvs_target_word_04=0x{:08x} misc_nvs_target_word_08=0x{:08x}",
        label,
        phase,
        rx_sta,
        rx_ap,
        read_u8(scan_ptr, 0x110),
        count_scan_history_nonzero_rows(scan_ptr),
        count_cnx_mgr_nonzero_slots(),
        count_cnx_mgr_seeded_slots(),
        read_u8(core::ptr::addr_of!(g_rssi_threshold_failure) as usize, 0),
        read_u8(core::ptr::addr_of!(g_authmode_threshold_failure) as usize, 0),
        read_u8(core::ptr::addr_of!(g_authmode_incompatible) as usize, 0),
        read_u8(core::ptr::addr_of!(g_in_blacklist_flag) as usize, 0),
        read_u8(g_ic_ptr, 0x18),
        read_u8(g_ic_ptr, 0x29f),
        read_u8(g_ic_ptr, 0x2a5),
        read_u8(wifi_nvs_ptr, 0x361),
        read_u8(wifi_nvs_ptr, 0x364),
        read_u8(wifi_nvs_ptr, 0x415),
        read_u8(wifi_nvs_ptr, 0x417),
        read_u8(wifi_nvs_ptr, 0x418),
        read_u32(g_misc_nvs_slot_ptr, 0x00),
        g_misc_nvs_target_ptr,
        read_u32(g_misc_nvs_target_ptr, 0x00),
        read_u32(g_misc_nvs_target_ptr, 0x04),
        read_u32(g_misc_nvs_target_ptr, 0x08),
    );
    let ic_1b4_ptr = read_ptr(g_ic_ptr, 0x1b4);
    println!(
        "legacy_nostd_wifi_control: scan_prelink_ic_1b4_fields label={} phase={} ptr=0x{:08x} bytes_34_3c={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} bytes_5d_5f={:02x}:{:02x}:{:02x} bytes_7c_8c={:02x}:{:02x}:{:02x}:{:02x}",
        label,
        phase,
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
        "legacy_nostd_wifi_control: scan_prelink_ic_scratch label={} phase={} ptr=0x{:08x} bytes_00_0f={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        label,
        phase,
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
    println!(
        "legacy_nostd_wifi_control: scan_prelink_ic_parse label={} phase={} bytes_200_20a={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} bytes_29c_29f={:02x}:{:02x}:{:02x}:{:02x}",
        label,
        phase,
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
}
