use super::*;

unsafe extern "C" {
    static mut g_chm: u8;
    static mut g_scan: u8;
}

unsafe fn read_u32(ptr: usize, offset: usize) -> u32 {
    ((ptr + offset) as *const u32).read_unaligned()
}

unsafe fn read_u8(ptr: usize, offset: usize) -> u8 {
    *((ptr + offset) as *const u8)
}

unsafe fn read_u16(ptr: usize, offset: usize) -> u16 {
    ((ptr + offset) as *const u16).read_unaligned()
}

fn can_read_data_ptr(ptr: u32) -> bool {
    ptr != 0 && ptr < 0x4000_0000
}

pub(super) unsafe fn capture_bytes(ptr: usize) -> [u8; SCAN_START_ARG3_BYTES] {
    let mut out = [0u8; SCAN_START_ARG3_BYTES];
    if !(0x3ff0_0000..0x4000_0000).contains(&ptr) {
        return out;
    }
    let mut idx = 0usize;
    while idx < SCAN_START_ARG3_BYTES {
        out[idx] = ((ptr + idx) as *const u8).read_volatile();
        idx += 1;
    }
    out
}

pub(super) unsafe fn fill_scan_start_state(snap: &mut Snapshot, pre: bool) {
    let chm_slot_ptr = core::ptr::addr_of!(g_chm) as usize;
    let scan_slot_ptr = core::ptr::addr_of!(g_scan) as usize;
    let chm_ptr = read_u32(chm_slot_ptr, 0) as usize;
    let scan_ptr = read_u32(scan_slot_ptr, 0) as usize;
    let op_chan = read_u8(chm_ptr, 0x04);
    let home_chan = read_u8(chm_ptr, 0x18);
    let current_chan = read_u8(chm_ptr, 0x1a);
    let chm_ptr08 = read_u32(chm_ptr, 0x08);
    let chm_ptr0c = read_u32(chm_ptr, 0x0c);
    let scan_word00 = read_u32(scan_ptr, 0x00);
    let scan_word114 = read_u32(scan_ptr, 0x114);
    if pre {
        snap.scan_start_pre_chm_ptr = chm_ptr as u32;
        snap.scan_start_pre_scan_ptr = scan_ptr as u32;
        snap.scan_start_pre_op_chan = op_chan;
        snap.scan_start_pre_home_chan = home_chan;
        snap.scan_start_pre_current_chan = current_chan;
        snap.scan_start_pre_chm_ptr08 = chm_ptr08;
        snap.scan_start_pre_chm_ptr0c = chm_ptr0c;
        snap.scan_start_pre_scan_word00 = scan_word00;
        snap.scan_start_pre_scan_word114 = scan_word114;
    } else {
        snap.scan_start_post_chm_ptr = chm_ptr as u32;
        snap.scan_start_post_scan_ptr = scan_ptr as u32;
        snap.scan_start_post_op_chan = op_chan;
        snap.scan_start_post_home_chan = home_chan;
        snap.scan_start_post_current_chan = current_chan;
        snap.scan_start_post_chm_ptr08 = chm_ptr08;
        snap.scan_start_post_chm_ptr0c = chm_ptr0c;
        snap.scan_start_post_scan_word00 = scan_word00;
        snap.scan_start_post_scan_word114 = scan_word114;
    }
}

pub(super) unsafe fn fill_scan_get_id_state(snap: &mut Snapshot) {
    let timer_diag = esp_radio::diagnostic_timer_compat_diag();
    let chm_slot_ptr = core::ptr::addr_of!(g_chm) as usize;
    let scan_slot_ptr = core::ptr::addr_of!(g_scan) as usize;
    let chm_ptr = read_u32(chm_slot_ptr, 0) as usize;
    let scan_ptr = read_u32(scan_slot_ptr, 0) as usize;
    snap.scan_get_id_exec_count = timer_diag.exec_count;
    snap.scan_get_id_last_callback_ptr = timer_diag.last_callback_ptr as u32;
    snap.scan_get_id_last_arg_ptr = timer_diag.last_arg_ptr as u32;
    snap.scan_get_id_op_chan = read_u8(chm_ptr, 0x04);
    snap.scan_get_id_scan_word114 = read_u32(scan_ptr, 0x114);
}

pub(super) unsafe fn fill_post_obj_fields(snap: &mut Snapshot, ptr: usize) {
    snap.obj_ptr = ptr as u32;
    snap.obj_word0 = read_u32(ptr, 0);
    snap.obj_word4 = read_u32(ptr, 4);
    snap.obj_byte8 = read_u8(ptr, 8);
    snap.obj_byte9 = read_u8(ptr, 9);
    snap.obj_word12 = read_u32(ptr, 12);
    snap.obj_word16 = read_u32(ptr, 16);
    snap.obj_word20 = read_u32(ptr, 20);
    snap.obj_word24 = read_u32(ptr, 24);
    snap.obj_byte28 = read_u8(ptr, 28);
    snap.obj_word32 = read_u32(ptr, 32);
    snap.obj_word36 = read_u32(ptr, 36);
    snap.obj_byte40 = read_u8(ptr, 40);
    if can_read_data_ptr(snap.obj_word4) {
        let ptr = snap.obj_word4 as usize;
        snap.ptr4_word0 = read_u32(ptr, 0);
        snap.ptr4_word4 = read_u32(ptr, 4);
        snap.ptr4_word8 = read_u32(ptr, 8);
        snap.ptr4_word12 = read_u32(ptr, 12);
    }
    if can_read_data_ptr(snap.obj_word16) {
        let ptr = snap.obj_word16 as usize;
        snap.ptr16_word0 = read_u32(ptr, 0);
        snap.ptr16_word4 = read_u32(ptr, 4);
        snap.ptr16_word8 = read_u32(ptr, 8);
        snap.ptr16_word12 = read_u32(ptr, 12);
    }
    let cmd = ptr + 20;
    snap.cmd_ptr = cmd as u32;
    snap.cmd_word0 = read_u32(cmd, 0);
    snap.cmd_word4 = read_u32(cmd, 4);
    snap.cmd_byte8 = read_u8(cmd, 8);
    snap.cmd_byte9 = read_u8(cmd, 9);
    snap.cmd_word12 = read_u32(cmd, 12);
    snap.cmd_word16 = read_u32(cmd, 16);
    snap.cmd_word20 = read_u32(cmd, 20);
    snap.cmd_word24 = read_u32(cmd, 24);
    snap.cmd_byte28 = read_u8(cmd, 28);
    snap.cmd_half32 = read_u16(cmd, 32);
    snap.cmd_word36 = read_u32(cmd, 36);
    snap.cmd_word40 = read_u32(cmd, 40);
}

pub(super) unsafe fn fill_pre_obj_fields(snap: &mut Snapshot, ptr: usize) {
    snap.pre_obj_ptr = ptr as u32;
    snap.pre_obj_word0 = read_u32(ptr, 0);
    snap.pre_obj_byte8 = read_u8(ptr, 8);
    snap.pre_obj_byte9 = read_u8(ptr, 9);
    snap.pre_obj_word12 = read_u32(ptr, 12);
    snap.pre_obj_word16 = read_u32(ptr, 16);
    snap.pre_obj_word20 = read_u32(ptr, 20);
    snap.pre_obj_word24 = read_u32(ptr, 24);
    snap.pre_obj_byte28 = read_u8(ptr, 28);
    snap.pre_obj_word32 = read_u32(ptr, 32);
    snap.pre_obj_word36 = read_u32(ptr, 36);
    snap.pre_obj_byte40 = read_u8(ptr, 40);
    if can_read_data_ptr(snap.pre_obj_word16) {
        let ptr = snap.pre_obj_word16 as usize;
        snap.pre_ptr16_word0 = read_u32(ptr, 0);
        snap.pre_ptr16_word4 = read_u32(ptr, 4);
        snap.pre_ptr16_word8 = read_u32(ptr, 8);
        snap.pre_ptr16_word12 = read_u32(ptr, 12);
    }
    let cmd = ptr + 20;
    snap.pre_cmd_ptr = cmd as u32;
    snap.pre_cmd_word0 = read_u32(cmd, 0);
    snap.pre_cmd_word4 = read_u32(cmd, 4);
    snap.pre_cmd_byte8 = read_u8(cmd, 8);
    snap.pre_cmd_byte9 = read_u8(cmd, 9);
    snap.pre_cmd_word12 = read_u32(cmd, 12);
    snap.pre_cmd_word16 = read_u32(cmd, 16);
    snap.pre_cmd_word20 = read_u32(cmd, 20);
    snap.pre_cmd_word24 = read_u32(cmd, 24);
    snap.pre_cmd_byte28 = read_u8(cmd, 28);
    snap.pre_cmd_half32 = read_u16(cmd, 32);
    snap.pre_cmd_word36 = read_u32(cmd, 36);
    snap.pre_cmd_word40 = read_u32(cmd, 40);
}
