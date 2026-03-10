use esp_println::println;

pub(super) fn print_timer_compat_diag(label: &str) {
    let diag = esp_radio::diagnostic_timer_compat_diag();
    println!(
        "nostd_wifi_control: timer_compat_diag label={} setfn_count={} arm_count={} exec_count={} wrapper_arm_count={} last_callback_ptr=0x{:08x} last_arg_ptr=0x{:08x} last_arm_us={} last_arm_repeat={} legacy_enabled={}",
        label,
        diag.setfn_count,
        diag.arm_count,
        diag.exec_count,
        diag.wrapper_arm_count,
        diag.last_callback_ptr,
        diag.last_arg_ptr,
        diag.last_arm_us,
        diag.last_arm_repeat,
        esp_radio::__esp_radio_diag_legacy_timer_compat_enabled(),
    );
}
