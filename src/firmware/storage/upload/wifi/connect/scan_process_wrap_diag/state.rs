use super::*;
use super::capture::{
    capture_bytes, fill_post_obj_fields, fill_pre_obj_fields, fill_scan_get_id_state,
    fill_scan_start_state,
};

fn store_snapshot(fn_id: u8, args: [usize; 4], ret: usize) {
    let slot = NEXT_SLOT.fetch_add(1, Ordering::Relaxed);
    if slot >= SLOT_COUNT {
        return;
    }
    let mut snap = Snapshot {
        fn_id,
        args: [
            args[0] as u32,
            args[1] as u32,
            args[2] as u32,
            args[3] as u32,
        ],
        ret: ret as u32,
        ..Snapshot::ZERO
    };
    if (fn_id == 7 || fn_id == 10) && args[0] != 0 {
        unsafe {
            fill_post_obj_fields(&mut snap, args[0]);
        }
    }
    unsafe {
        SNAPSHOTS[slot] = snap;
    }
}

pub(super) unsafe fn wrap_call(
    fn_id: u8,
    real: unsafe extern "C" fn(usize, usize, usize, usize, usize, usize) -> usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    let mut pre = Snapshot::ZERO;
    if fn_id == 2 {
        pre.scan_start_pre_arg3 = unsafe { capture_bytes(a5) };
        unsafe { fill_scan_start_state(&mut pre, true) };
    }
    if (fn_id == 7 || fn_id == 10) && a2 != 0 {
        unsafe {
            fill_pre_obj_fields(&mut pre, a2);
        }
    }
    let ret = unsafe { real(a2, a3, a4, a5, a6, a7) };
    store_snapshot(fn_id, [a2, a3, a4, a5], ret);
    if fn_id == 2 || ((fn_id == 7 || fn_id == 10) && a2 != 0) {
        let slot = NEXT_SLOT.load(Ordering::Relaxed).saturating_sub(1);
        if slot < SLOT_COUNT {
            unsafe {
                if fn_id == 2 {
                    SNAPSHOTS[slot].scan_start_pre_arg3 = pre.scan_start_pre_arg3;
                    SNAPSHOTS[slot].scan_start_post_arg3 = capture_bytes(a5);
                    fill_scan_start_state(&mut SNAPSHOTS[slot], false);
                    SNAPSHOTS[slot].scan_start_pre_op_chan = pre.scan_start_pre_op_chan;
                    SNAPSHOTS[slot].scan_start_pre_current_chan = pre.scan_start_pre_current_chan;
                    SNAPSHOTS[slot].scan_start_pre_home_chan = pre.scan_start_pre_home_chan;
                    SNAPSHOTS[slot].scan_start_pre_scan_word00 = pre.scan_start_pre_scan_word00;
                    SNAPSHOTS[slot].scan_start_pre_scan_word114 = pre.scan_start_pre_scan_word114;
                    SNAPSHOTS[slot].scan_start_pre_scan_ptr = pre.scan_start_pre_scan_ptr;
                    SNAPSHOTS[slot].scan_start_pre_chm_ptr = pre.scan_start_pre_chm_ptr;
                    SNAPSHOTS[slot].scan_start_pre_chm_ptr08 = pre.scan_start_pre_chm_ptr08;
                    SNAPSHOTS[slot].scan_start_pre_chm_ptr0c = pre.scan_start_pre_chm_ptr0c;
                } else if fn_id == 5 {
                    fill_scan_get_id_state(&mut SNAPSHOTS[slot]);
                }
                SNAPSHOTS[slot].pre_obj_ptr = pre.pre_obj_ptr;
                SNAPSHOTS[slot].pre_obj_word0 = pre.pre_obj_word0;
                SNAPSHOTS[slot].pre_obj_byte8 = pre.pre_obj_byte8;
                SNAPSHOTS[slot].pre_obj_byte9 = pre.pre_obj_byte9;
                SNAPSHOTS[slot].pre_obj_word12 = pre.pre_obj_word12;
                SNAPSHOTS[slot].pre_obj_word16 = pre.pre_obj_word16;
                SNAPSHOTS[slot].pre_obj_word20 = pre.pre_obj_word20;
                SNAPSHOTS[slot].pre_obj_word24 = pre.pre_obj_word24;
                SNAPSHOTS[slot].pre_obj_byte28 = pre.pre_obj_byte28;
                SNAPSHOTS[slot].pre_obj_word32 = pre.pre_obj_word32;
                SNAPSHOTS[slot].pre_obj_word36 = pre.pre_obj_word36;
                SNAPSHOTS[slot].pre_obj_byte40 = pre.pre_obj_byte40;
                SNAPSHOTS[slot].pre_ptr16_word0 = pre.pre_ptr16_word0;
                SNAPSHOTS[slot].pre_ptr16_word4 = pre.pre_ptr16_word4;
                SNAPSHOTS[slot].pre_ptr16_word8 = pre.pre_ptr16_word8;
                SNAPSHOTS[slot].pre_ptr16_word12 = pre.pre_ptr16_word12;
                SNAPSHOTS[slot].pre_cmd_ptr = pre.pre_cmd_ptr;
                SNAPSHOTS[slot].pre_cmd_word0 = pre.pre_cmd_word0;
                SNAPSHOTS[slot].pre_cmd_word4 = pre.pre_cmd_word4;
                SNAPSHOTS[slot].pre_cmd_byte8 = pre.pre_cmd_byte8;
                SNAPSHOTS[slot].pre_cmd_byte9 = pre.pre_cmd_byte9;
                SNAPSHOTS[slot].pre_cmd_word12 = pre.pre_cmd_word12;
                SNAPSHOTS[slot].pre_cmd_word16 = pre.pre_cmd_word16;
                SNAPSHOTS[slot].pre_cmd_word20 = pre.pre_cmd_word20;
                SNAPSHOTS[slot].pre_cmd_word24 = pre.pre_cmd_word24;
                SNAPSHOTS[slot].pre_cmd_byte28 = pre.pre_cmd_byte28;
                SNAPSHOTS[slot].pre_cmd_half32 = pre.pre_cmd_half32;
                SNAPSHOTS[slot].pre_cmd_word36 = pre.pre_cmd_word36;
                SNAPSHOTS[slot].pre_cmd_word40 = pre.pre_cmd_word40;
            }
        }
    }
    ret
}
