use esp_println::println;

pub(super) fn log_legacy_preempt_builtin(stage: &str) {
    let diag = esp_radio::diagnostic_legacy_preempt_builtin_diag();
    println!(
        "upload_http: boot_scan_only_diag legacy_preempt_builtin after={} initialized={} current_task=0x{:x} current_task_thread_semaphore=0x{:x}",
        stage,
        diag.initialized as u8,
        diag.current_task,
        diag.current_task_thread_semaphore,
    );
}
