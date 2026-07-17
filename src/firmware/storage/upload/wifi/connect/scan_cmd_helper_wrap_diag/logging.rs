use super::*;
use esp_println::println;

fn fn_label(fn_id: u8) -> &'static str {
    match fn_id {
        1 => "scan_hidden_ssid",
        2 => "scan_set_current_scan_times",
        3 => "scan_build_chan_list",
        4 => "scan_set_desChan",
        5 => "ieee80211_regdomain_chan_in_range",
        6 => "ieee80211_regdomain_min_chan",
        7 => "ieee80211_regdomain_max_chan",
        _ => "unknown",
    }
}

pub(super) fn log(stage: &str) {
    let count = NEXT_SLOT.load(Ordering::Relaxed).min(SLOT_COUNT);
    println!("upload_http: boot_scan_only_diag scan_cmd_helper_wrap_diag after={} count={}", stage, count);
    for idx in 0..count {
        let snap = unsafe { SNAPSHOTS[idx] };
        println!(
            "upload_http: boot_scan_only_diag scan_cmd_helper_wrap_diag_entry after={} idx={} fn={} ret=0x{:08x} arg0=0x{:08x} arg1=0x{:08x} arg2=0x{:08x} call_arg2=0x{:08x} arg3=0x{:08x} pre_app_scan_params={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} post_app_scan_params={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} pre_arg2={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} post_arg2={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} pre_arg3={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} post_arg3={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            stage, idx, fn_label(snap.fn_id), snap.ret, snap.args[0], snap.args[1], snap.args[2], snap.call_arg2, snap.args[3],
            snap.pre_app_scan_params[0], snap.pre_app_scan_params[1], snap.pre_app_scan_params[2], snap.pre_app_scan_params[3], snap.pre_app_scan_params[4], snap.pre_app_scan_params[5], snap.pre_app_scan_params[6], snap.pre_app_scan_params[7], snap.pre_app_scan_params[8], snap.pre_app_scan_params[9], snap.pre_app_scan_params[10], snap.pre_app_scan_params[11], snap.pre_app_scan_params[12], snap.pre_app_scan_params[13], snap.pre_app_scan_params[14], snap.pre_app_scan_params[15],
            snap.post_app_scan_params[0], snap.post_app_scan_params[1], snap.post_app_scan_params[2], snap.post_app_scan_params[3], snap.post_app_scan_params[4], snap.post_app_scan_params[5], snap.post_app_scan_params[6], snap.post_app_scan_params[7], snap.post_app_scan_params[8], snap.post_app_scan_params[9], snap.post_app_scan_params[10], snap.post_app_scan_params[11], snap.post_app_scan_params[12], snap.post_app_scan_params[13], snap.post_app_scan_params[14], snap.post_app_scan_params[15],
            snap.pre_arg2[0], snap.pre_arg2[1], snap.pre_arg2[2], snap.pre_arg2[3], snap.pre_arg2[4], snap.pre_arg2[5], snap.pre_arg2[6], snap.pre_arg2[7],
            snap.post_arg2[0], snap.post_arg2[1], snap.post_arg2[2], snap.post_arg2[3], snap.post_arg2[4], snap.post_arg2[5], snap.post_arg2[6], snap.post_arg2[7],
            snap.pre_arg3[0], snap.pre_arg3[1], snap.pre_arg3[2], snap.pre_arg3[3], snap.pre_arg3[4], snap.pre_arg3[5], snap.pre_arg3[6], snap.pre_arg3[7],
            snap.post_arg3[0], snap.post_arg3[1], snap.post_arg3[2], snap.post_arg3[3], snap.post_arg3[4], snap.post_arg3[5], snap.post_arg3[6], snap.post_arg3[7],
        );
    }
}
