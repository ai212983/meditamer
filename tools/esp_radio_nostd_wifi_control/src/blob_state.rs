use esp_println::println;

unsafe extern "C" {
    static mut g_chm: u8;
    static mut g_scan: u8;
    static mut g_wifi_nvs: u8;
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
    unsafe { ((base + offset) as *const u32).read_volatile() }
}

fn read_ptr(base: usize, offset: usize) -> usize {
    read_u32(base, offset) as usize
}

pub(crate) fn print_blob_state(label: &str) {
    let g_chm_slot_ptr = core::ptr::addr_of!(g_chm) as usize;
    let g_scan_slot_ptr = core::ptr::addr_of!(g_scan) as usize;
    let g_wifi_nvs_slot_ptr = core::ptr::addr_of!(g_wifi_nvs) as usize;
    let chm_ptr = read_ptr(g_chm_slot_ptr, 0x0);
    let scan_ptr = read_ptr(g_scan_slot_ptr, 0x0);
    let wifi_nvs_ptr = read_ptr(g_wifi_nvs_slot_ptr, 0x0);

    println!(
        "nostd_wifi_control: blob_state label={} g_wifi_nvs_slot_ptr=0x{:08x} g_wifi_nvs_ptr=0x{:08x} wifi_nvs_byte_00=0x{:02x} wifi_nvs_byte_35c=0x{:02x}",
        label,
        g_wifi_nvs_slot_ptr,
        wifi_nvs_ptr,
        read_u8(wifi_nvs_ptr, 0),
        read_u8(wifi_nvs_ptr, 0x35c),
    );
    println!(
        "nostd_wifi_control: blob_chm label={} chm_ptr=0x{:08x} op_chan=0x{:02x} op_mode=0x{:02x} ptr_08=0x{:x} ptr_0c=0x{:x} home_chan=0x{:02x} current_chan=0x{:02x}",
        label,
        chm_ptr,
        read_u8(chm_ptr, 0x04),
        read_u8(chm_ptr, 0x05),
        read_ptr(chm_ptr, 0x08),
        read_ptr(chm_ptr, 0x0c),
        read_u8(chm_ptr, 0x50),
        read_u8(chm_ptr, 0x52),
    );
    println!(
        "nostd_wifi_control: blob_scan label={} scan_ptr=0x{:08x} word_30=0x{:08x} word_34=0x{:08x} byte_44=0x{:02x} byte_45=0x{:02x} flags_70=0x{:02x} flags_71=0x{:02x} word_114=0x{:08x}",
        label,
        scan_ptr,
        read_u32(scan_ptr, 0x30),
        read_u32(scan_ptr, 0x34),
        read_u8(scan_ptr, 0x44),
        read_u8(scan_ptr, 0x45),
        read_u8(scan_ptr, 0x46),
        read_u8(scan_ptr, 0x47),
        read_u32(scan_ptr, 0x114),
    );
}
