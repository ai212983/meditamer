use super::*;
use esp_println::println;

pub(super) fn reset() {
    NEXT_SLOT.store(0, Ordering::Relaxed);
    unsafe {
        let mut idx = 0usize;
        while idx < SLOT_COUNT {
            SNAPSHOTS[idx] = Snapshot::ZERO;
            idx += 1;
        }
    }
}

fn fn_label(fn_id: u8) -> &'static str {
    match fn_id {
        1 => "scan_pm_offchan",
        2 => "scan_start",
        3 => "scan_start_handler",
        4 => "scan_set_scan_id",
        5 => "scan_get_scan_id",
        6 => "scan_enter_oper_channel_process",
        7 => "wifi_scan_start_process",
        8 => "scan_inter_channel_timeout_process",
        9 => "clear_bss_queue",
        10 => "ieee80211_sta_scan",
        _ => "unknown",
    }
}

pub(super) fn log(stage: &str) {
    let count = NEXT_SLOT.load(Ordering::Relaxed).min(SLOT_COUNT);
    println!(
        "upload_http: boot_scan_only_diag scan_process_wrap_diag after={} count={}",
        stage, count
    );
    for idx in 0..count {
        let snap = unsafe { SNAPSHOTS[idx] };
        println!(
            "upload_http: boot_scan_only_diag scan_process_wrap_diag_entry after={} idx={} fn={} ret=0x{:08x} arg0=0x{:08x} arg1=0x{:08x} arg2=0x{:08x} arg3=0x{:08x}",
            stage,
            idx,
            fn_label(snap.fn_id),
            snap.ret,
            snap.args[0],
            snap.args[1],
            snap.args[2],
            snap.args[3],
        );
        if snap.fn_id == 2 {
            println!(
                "upload_http: boot_scan_only_diag scan_process_wrap_scan_start_arg3 after={} idx={} pre={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} post={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                stage,
                idx,
                snap.scan_start_pre_arg3[0], snap.scan_start_pre_arg3[1], snap.scan_start_pre_arg3[2], snap.scan_start_pre_arg3[3],
                snap.scan_start_pre_arg3[4], snap.scan_start_pre_arg3[5], snap.scan_start_pre_arg3[6], snap.scan_start_pre_arg3[7],
                snap.scan_start_pre_arg3[8], snap.scan_start_pre_arg3[9], snap.scan_start_pre_arg3[10], snap.scan_start_pre_arg3[11],
                snap.scan_start_pre_arg3[12], snap.scan_start_pre_arg3[13], snap.scan_start_pre_arg3[14], snap.scan_start_pre_arg3[15],
                snap.scan_start_post_arg3[0], snap.scan_start_post_arg3[1], snap.scan_start_post_arg3[2], snap.scan_start_post_arg3[3],
                snap.scan_start_post_arg3[4], snap.scan_start_post_arg3[5], snap.scan_start_post_arg3[6], snap.scan_start_post_arg3[7],
                snap.scan_start_post_arg3[8], snap.scan_start_post_arg3[9], snap.scan_start_post_arg3[10], snap.scan_start_post_arg3[11],
                snap.scan_start_post_arg3[12], snap.scan_start_post_arg3[13], snap.scan_start_post_arg3[14], snap.scan_start_post_arg3[15],
            );
            println!(
                "upload_http: boot_scan_only_diag scan_process_wrap_scan_start_state after={} idx={} pre_chm_ptr=0x{:08x} post_chm_ptr=0x{:08x} pre_scan_ptr=0x{:08x} post_scan_ptr=0x{:08x} pre_op_chan=0x{:02x} post_op_chan=0x{:02x} pre_home_chan=0x{:02x} post_home_chan=0x{:02x} pre_current_chan=0x{:02x} post_current_chan=0x{:02x} pre_ptr08=0x{:08x} post_ptr08=0x{:08x} pre_ptr0c=0x{:08x} post_ptr0c=0x{:08x} pre_scan_word00=0x{:08x} post_scan_word00=0x{:08x} pre_scan_word114=0x{:08x} post_scan_word114=0x{:08x}",
                stage,
                idx,
                snap.scan_start_pre_chm_ptr,
                snap.scan_start_post_chm_ptr,
                snap.scan_start_pre_scan_ptr,
                snap.scan_start_post_scan_ptr,
                snap.scan_start_pre_op_chan,
                snap.scan_start_post_op_chan,
                snap.scan_start_pre_home_chan,
                snap.scan_start_post_home_chan,
                snap.scan_start_pre_current_chan,
                snap.scan_start_post_current_chan,
                snap.scan_start_pre_chm_ptr08,
                snap.scan_start_post_chm_ptr08,
                snap.scan_start_pre_chm_ptr0c,
                snap.scan_start_post_chm_ptr0c,
                snap.scan_start_pre_scan_word00,
                snap.scan_start_post_scan_word00,
                snap.scan_start_pre_scan_word114,
                snap.scan_start_post_scan_word114,
            );
        } else if snap.fn_id == 5 {
            println!(
                "upload_http: boot_scan_only_diag scan_process_wrap_scan_get_id_state after={} idx={} ret=0x{:08x} exec_count={} last_callback_ptr=0x{:08x} last_arg_ptr=0x{:08x} op_chan=0x{:02x} scan_word114=0x{:08x}",
                stage,
                idx,
                snap.ret,
                snap.scan_get_id_exec_count,
                snap.scan_get_id_last_callback_ptr,
                snap.scan_get_id_last_arg_ptr,
                snap.scan_get_id_op_chan,
                snap.scan_get_id_scan_word114,
            );
        }
        if (snap.fn_id == 7 || snap.fn_id == 10) && snap.obj_ptr != 0 {
            println!(
                "upload_http: boot_scan_only_diag scan_process_wrap_obj after={} idx={} fn={} pre_ptr=0x{:08x} pre_word0=0x{:08x} pre_byte8=0x{:02x} pre_byte9=0x{:02x} pre_word12=0x{:08x} pre_word16=0x{:08x} pre_word20=0x{:08x} pre_word24=0x{:08x} pre_byte28=0x{:02x} pre_word32=0x{:08x} pre_word36=0x{:08x} pre_byte40=0x{:02x} pre_ptr16=[0x{:08x},0x{:08x},0x{:08x},0x{:08x}] pre_cmd_ptr=0x{:08x} pre_cmd=[w0=0x{:08x},w4=0x{:08x},b8=0x{:02x},b9=0x{:02x},w12=0x{:08x},w16=0x{:08x},w20=0x{:08x},w24=0x{:08x},b28=0x{:02x},h32=0x{:04x},w36=0x{:08x},w40=0x{:08x}] ptr=0x{:08x} word0=0x{:08x} word4=0x{:08x} byte8=0x{:02x} byte9=0x{:02x} word12=0x{:08x} word16=0x{:08x} word20=0x{:08x} word24=0x{:08x} byte28=0x{:02x} word32=0x{:08x} word36=0x{:08x} byte40=0x{:02x} ptr4=[0x{:08x},0x{:08x},0x{:08x},0x{:08x}] ptr16=[0x{:08x},0x{:08x},0x{:08x},0x{:08x}] cmd_ptr=0x{:08x} cmd=[w0=0x{:08x},w4=0x{:08x},b8=0x{:02x},b9=0x{:02x},w12=0x{:08x},w16=0x{:08x},w20=0x{:08x},w24=0x{:08x},b28=0x{:02x},h32=0x{:04x},w36=0x{:08x},w40=0x{:08x}]",
                stage,
                idx,
                fn_label(snap.fn_id),
                snap.pre_obj_ptr,
                snap.pre_obj_word0,
                snap.pre_obj_byte8,
                snap.pre_obj_byte9,
                snap.pre_obj_word12,
                snap.pre_obj_word16,
                snap.pre_obj_word20,
                snap.pre_obj_word24,
                snap.pre_obj_byte28,
                snap.pre_obj_word32,
                snap.pre_obj_word36,
                snap.pre_obj_byte40,
                snap.pre_ptr16_word0,
                snap.pre_ptr16_word4,
                snap.pre_ptr16_word8,
                snap.pre_ptr16_word12,
                snap.pre_cmd_ptr,
                snap.pre_cmd_word0,
                snap.pre_cmd_word4,
                snap.pre_cmd_byte8,
                snap.pre_cmd_byte9,
                snap.pre_cmd_word12,
                snap.pre_cmd_word16,
                snap.pre_cmd_word20,
                snap.pre_cmd_word24,
                snap.pre_cmd_byte28,
                snap.pre_cmd_half32,
                snap.pre_cmd_word36,
                snap.pre_cmd_word40,
                snap.obj_ptr,
                snap.obj_word0,
                snap.obj_word4,
                snap.obj_byte8,
                snap.obj_byte9,
                snap.obj_word12,
                snap.obj_word16,
                snap.obj_word20,
                snap.obj_word24,
                snap.obj_byte28,
                snap.obj_word32,
                snap.obj_word36,
                snap.obj_byte40,
                snap.ptr4_word0,
                snap.ptr4_word4,
                snap.ptr4_word8,
                snap.ptr4_word12,
                snap.ptr16_word0,
                snap.ptr16_word4,
                snap.ptr16_word8,
                snap.ptr16_word12,
                snap.cmd_ptr,
                snap.cmd_word0,
                snap.cmd_word4,
                snap.cmd_byte8,
                snap.cmd_byte9,
                snap.cmd_word12,
                snap.cmd_word16,
                snap.cmd_word20,
                snap.cmd_word24,
                snap.cmd_byte28,
                snap.cmd_half32,
                snap.cmd_word36,
                snap.cmd_word40,
            );
        }
    }
}
