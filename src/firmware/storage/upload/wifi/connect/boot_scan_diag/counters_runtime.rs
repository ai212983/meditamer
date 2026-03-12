use super::task_role::format_task_role;
use esp_println::println;

pub(super) fn log_boot_scan_only_runtime_counters(stage: &str) {
    let timer_diag = esp_radio::diagnostic_timer_compat_diag();
    println!(
        "upload_http: boot_scan_only_diag timer_compat_diag after={} setfn_count={} arm_count={} exec_count={} process_due_call_count={} process_due_hit_count={} wrapper_arm_count={} last_ets_timer_ptr=0x{:x} last_timer_handle_ptr=0x{:x} last_callback_ptr=0x{:x} last_arg_ptr=0x{:x} last_arm_us={} last_arm_repeat={} last_now_us={} last_started_us={} last_timeout_us={} last_next_due_us={} suppressed_setfn_count={} last_suppressed_setfn_callback_ptr=0x{:x} last_suppressed_setfn_arg_ptr=0x{:x} suppressed_arm_count={} last_suppressed_callback_ptr=0x{:x} last_suppressed_arg_ptr=0x{:x} last_suppressed_us={}",
        stage,
        timer_diag.setfn_count,
        timer_diag.arm_count,
        timer_diag.exec_count,
        timer_diag.process_due_call_count,
        timer_diag.process_due_hit_count,
        timer_diag.wrapper_arm_count,
        timer_diag.last_ets_timer_ptr,
        timer_diag.last_timer_handle_ptr,
        timer_diag.last_callback_ptr,
        timer_diag.last_arg_ptr,
        timer_diag.last_arm_us,
        timer_diag.last_arm_repeat,
        timer_diag.last_now_us,
        timer_diag.last_started_us,
        timer_diag.last_timeout_us,
        timer_diag.last_next_due_us,
        timer_diag.suppressed_setfn_count,
        timer_diag.last_suppressed_setfn_callback_ptr,
        timer_diag.last_suppressed_setfn_arg_ptr,
        timer_diag.suppressed_arm_count,
        timer_diag.last_suppressed_callback_ptr,
        timer_diag.last_suppressed_arg_ptr,
        timer_diag.last_suppressed_us,
    );
    let timer_exec_diag = esp_radio::diagnostic_timer_callback_exec_diag();
    println!(
        "upload_http: boot_scan_only_diag timer_exec_diag after={} current_callback_ptr=0x{:x} current_arg_ptr=0x{:x} last_callback_ptr=0x{:x} last_arg_ptr=0x{:x} last_exec_at_us={} last_due_at_us={} last_timeout_us={} last_lateness_us={} max_lateness_us={}",
        stage,
        timer_exec_diag.current_callback_ptr,
        timer_exec_diag.current_arg_ptr,
        timer_exec_diag.last_callback_ptr,
        timer_exec_diag.last_arg_ptr,
        timer_exec_diag.last_exec_at_us,
        timer_exec_diag.last_due_at_us,
        timer_exec_diag.last_timeout_us,
        timer_exec_diag.last_lateness_us,
        timer_exec_diag.max_lateness_us,
    );
    let timer_runtime_diag = esp_radio::diagnostic_timer_task_runtime_diag();
    println!(
        "upload_http: boot_scan_only_diag timer_runtime_diag after={} entry_count={} resume_count={} loop_count={} legacy_compat_branch_count={} legacy_driver_branch_count={} default_branch_count={} mark_ready_count={} pop_count={} selected_count={} task_ptr=0x{:x} legacy_compat_enabled={}",
        stage,
        timer_runtime_diag.entry_count,
        timer_runtime_diag.resume_count,
        timer_runtime_diag.loop_count,
        timer_runtime_diag.legacy_compat_branch_count,
        timer_runtime_diag.legacy_driver_branch_count,
        timer_runtime_diag.default_branch_count,
        timer_runtime_diag.mark_ready_count,
        timer_runtime_diag.pop_count,
        timer_runtime_diag.selected_count,
        timer_runtime_diag.task_ptr,
        timer_runtime_diag.legacy_compat_enabled,
    );
    let legacy_task_model_diag = esp_radio::diagnostic_legacy_task_model_diag();
    println!(
        "upload_http: boot_scan_only_diag legacy_task_model after={} entry_count={} current_index={} last_pop_candidate_ptr=0x{:x} last_pop_candidate_state={} last_pop_selected_ptr=0x{:x}",
        stage,
        legacy_task_model_diag.entry_count,
        legacy_task_model_diag.current_index,
        legacy_task_model_diag.last_pop_candidate_ptr,
        legacy_task_model_diag.last_pop_candidate_state,
        legacy_task_model_diag.last_pop_selected_ptr,
    );
    for idx in 0..legacy_task_model_diag.task_ptrs.len() {
        let task_ptr = legacy_task_model_diag.task_ptrs[idx];
        if task_ptr != 0 {
            println!(
                "upload_http: boot_scan_only_diag legacy_task_model_slot after={} idx={} task_ptr=0x{:x} task_role={} state={}",
                stage,
                idx,
                task_ptr,
                format_task_role(task_ptr),
                legacy_task_model_diag.task_states[idx],
            );
        }
    }
    let legacy_builtin_diag = esp_radio::diagnostic_legacy_builtin_scheduler_diag();
    println!(
        "upload_http: boot_scan_only_diag legacy_builtin_scheduler after={} initialized={} current_task=0x{:x} to_delete=0x{:x} switch_count={} last_selected_task=0x{:x}",
        stage,
        legacy_builtin_diag.initialized as u8,
        legacy_builtin_diag.current_task,
        legacy_builtin_diag.to_delete,
        legacy_builtin_diag.switch_count,
        legacy_builtin_diag.last_selected_task,
    );
    for idx in 0..legacy_builtin_diag.task_ptrs.len() {
        let task_ptr = legacy_builtin_diag.task_ptrs[idx];
        if task_ptr != 0 {
            let role = core::str::from_utf8(&legacy_builtin_diag.task_roles[idx])
                .ok()
                .map(|s| s.trim_end_matches('\0'))
                .unwrap_or("<invalid>");
            println!(
                "upload_http: boot_scan_only_diag legacy_builtin_scheduler_slot after={} idx={} task_ptr=0x{:x} task_role={}{}",
                stage,
                idx,
                task_ptr,
                role,
                if task_ptr == legacy_builtin_diag.current_task {
                    " current=true"
                } else if task_ptr == legacy_builtin_diag.last_selected_task {
                    " last_selected=true"
                } else {
                    ""
                },
            );
        }
    }
    for idx in 0..timer_diag.recent_setfn_ordinals.len() {
        let ordinal = timer_diag.recent_setfn_ordinals[idx];
        if ordinal != 0 {
            println!(
                "upload_http: boot_scan_only_diag timer_compat_setfn_recent after={} idx={} ordinal={} ets_timer_ptr=0x{:x} timer_handle_ptr=0x{:x} callback_ptr=0x{:x} arg_ptr=0x{:x}",
                stage,
                idx,
                ordinal,
                timer_diag.recent_setfn_ets_timer_ptrs[idx],
                timer_diag.recent_setfn_timer_handle_ptrs[idx],
                timer_diag.recent_setfn_callback_ptrs[idx],
                timer_diag.recent_setfn_arg_ptrs[idx],
            );
        }
    }
    for idx in 0..timer_diag.recent_arm_ordinals.len() {
        let ordinal = timer_diag.recent_arm_ordinals[idx];
        if ordinal != 0 {
            println!(
                "upload_http: boot_scan_only_diag timer_compat_arm_recent after={} idx={} ordinal={} ets_timer_ptr=0x{:x} timer_handle_ptr=0x{:x} callback_ptr=0x{:x} arg_ptr=0x{:x} caller_ptr=0x{:x} us={} repeat={}",
                stage,
                idx,
                ordinal,
                timer_diag.recent_arm_ets_timer_ptrs[idx],
                timer_diag.recent_arm_timer_handle_ptrs[idx],
                timer_diag.recent_arm_callback_ptrs[idx],
                timer_diag.recent_arm_arg_ptrs[idx],
                timer_diag.recent_arm_caller_ptrs[idx],
                timer_diag.recent_arm_us[idx],
                timer_diag.recent_arm_repeat[idx],
            );
        }
    }
    for idx in 0..timer_diag.recent_wrapper_arm_ordinals.len() {
        let ordinal = timer_diag.recent_wrapper_arm_ordinals[idx];
        if ordinal != 0 {
            println!(
                "upload_http: boot_scan_only_diag timer_compat_wrapper_arm_recent after={} idx={} ordinal={} timer_ptr=0x{:x} caller_ptr=0x{:x} us={} repeat={}",
                stage,
                idx,
                ordinal,
                timer_diag.recent_wrapper_arm_timer_ptrs[idx],
                timer_diag.recent_wrapper_arm_caller_ptrs[idx],
                timer_diag.recent_wrapper_arm_us[idx],
                timer_diag.recent_wrapper_arm_repeat[idx],
            );
        }
    }
    let legacy_preempt_diag = esp_radio::diagnostic_legacy_preempt_builtin_diag();
    println!(
        "upload_http: boot_scan_only_diag legacy_preempt_builtin after={} initialized={} current_task=0x{:x} current_task_thread_semaphore=0x{:x}",
        stage,
        legacy_preempt_diag.initialized as u8,
        legacy_preempt_diag.current_task,
        legacy_preempt_diag.current_task_thread_semaphore,
    );
}
