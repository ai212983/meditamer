use super::{log_legacy_preempt_builtin, task_role::format_task_role};
use esp_println::println;

pub(super) fn log_boot_scan_only_runtime_counters(stage: &str) {
    let init_config = esp_radio::diagnostic_wifi_init_config_diag();
    println!(
        "upload_http: boot_scan_only_diag wifi_init_config after={} config_ptr=0x{:08x} osi_funcs_ptr=0x{:08x} set_intr=0x{:08x} clear_intr=0x{:08x} set_isr=0x{:08x} ints_on=0x{:08x} ints_off=0x{:08x} wifi_int_disable=0x{:08x} wifi_int_restore=0x{:08x} task_yield_from_isr=0x{:08x} queue_send_from_isr=0x{:08x} queue_create=0x{:08x} queue_recv=0x{:08x}",
        stage,
        init_config.config_ptr,
        init_config.osi_funcs_ptr,
        init_config.osi_set_intr_ptr,
        init_config.osi_clear_intr_ptr,
        init_config.osi_set_isr_ptr,
        init_config.osi_ints_on_ptr,
        init_config.osi_ints_off_ptr,
        init_config.osi_wifi_int_disable_ptr,
        init_config.osi_wifi_int_restore_ptr,
        init_config.osi_task_yield_from_isr_ptr,
        init_config.osi_queue_send_from_isr_ptr,
        init_config.osi_queue_create_ptr,
        init_config.osi_queue_recv_ptr,
    );
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
        "upload_http: boot_scan_only_diag timer_exec_diag after={} current_callback_ptr=0x{:x} current_arg_ptr=0x{:x} last_callback_ptr=0x{:x} last_arg_ptr=0x{:x} last_exec_at_us={} last_due_at_us={} last_timeout_us={} last_lateness_us={} max_lateness_us={} sideeffect_arm_count={} sideeffect_disarm_count={} sideeffect_last_kind={} sideeffect_last_current_ptr=0x{:x} sideeffect_last_current_arg_ptr=0x{:x} sideeffect_last_target_timer_ptr=0x{:x} sideeffect_last_target_callback_ptr=0x{:x} sideeffect_last_target_arg_ptr=0x{:x} sideeffect_last_timeout_us={} sideeffect_last_repeat={}",
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
        timer_exec_diag.sideeffect_arm_count,
        timer_exec_diag.sideeffect_disarm_count,
        timer_exec_diag.sideeffect_last_kind,
        timer_exec_diag.sideeffect_last_current_ptr,
        timer_exec_diag.sideeffect_last_current_arg_ptr,
        timer_exec_diag.sideeffect_last_target_timer_ptr,
        timer_exec_diag.sideeffect_last_target_callback_ptr,
        timer_exec_diag.sideeffect_last_target_arg_ptr,
        timer_exec_diag.sideeffect_last_timeout_us,
        timer_exec_diag.sideeffect_last_repeat as u8,
    );
    for idx in 0..timer_exec_diag.recent_ordinals.len() {
        let ordinal = timer_exec_diag.recent_ordinals[idx];
        if ordinal != 0 {
            println!(
                "upload_http: boot_scan_only_diag timer_exec_recent after={} idx={} ordinal={} callback_ptr=0x{:x} arg_ptr=0x{:x} exec_at_us={} due_at_us={} timeout_us={} lateness_us={}",
                stage,
                idx,
                ordinal,
                timer_exec_diag.recent_callback_ptrs[idx],
                timer_exec_diag.recent_arg_ptrs[idx],
                timer_exec_diag.recent_exec_at_us[idx],
                timer_exec_diag.recent_due_at_us[idx],
                timer_exec_diag.recent_timeout_us[idx],
                timer_exec_diag.recent_lateness_us[idx],
            );
        }
    }
    for idx in 0..timer_diag.recent_due_ordinals.len() {
        let ordinal = timer_diag.recent_due_ordinals[idx];
        if ordinal != 0 {
            println!(
                "upload_http: boot_scan_only_diag timer_compat_due_recent after={} idx={} ordinal={} found={} executed={} callback_ptr=0x{:x} arg_ptr=0x{:x} pre_op_chan=0x{:02x} pre_scan_word00=0x{:08x} pre_scan_word114=0x{:08x}",
                stage,
                idx,
                ordinal,
                timer_diag.recent_due_found[idx],
                timer_diag.recent_due_executed[idx],
                timer_diag.recent_due_callback_ptrs[idx],
                timer_diag.recent_due_arg_ptrs[idx],
                timer_diag.recent_due_op_chans[idx],
                timer_diag.recent_due_scan_word00[idx],
                timer_diag.recent_due_scan_word114[idx],
            );
        }
    }
    let timer_runtime_diag = esp_radio::diagnostic_timer_task_runtime_diag();
    println!(
        "upload_http: boot_scan_only_diag timer_runtime_diag after={} entry_count={} resume_count={} loop_count={} legacy_compat_branch_count={} legacy_driver_branch_count={} default_branch_count={} create_count={} create_from_ensure_count={} create_from_wake_count={} create_from_enqueue_count={} create_last_mode={} create_last_source={} create_last_ptr=0x{:x} process_skip_inactive_count={} process_skip_not_due_count={} process_last_skip_callback_ptr=0x{:x} process_last_skip_arg_ptr=0x{:x} process_last_skip_now_us={} process_last_skip_due_us={} mark_ready_count={} pop_count={} selected_count={} sleep_count={} sleep_true_count={} sleep_false_count={} sleep_last_task_ptr=0x{:x} sleep_last_wake_at_us={} sleep_last_result={} sleep_task_mismatch_count={} task_ptr=0x{:x} legacy_compat_enabled={}",
        stage,
        timer_runtime_diag.entry_count,
        timer_runtime_diag.resume_count,
        timer_runtime_diag.loop_count,
        timer_runtime_diag.legacy_compat_branch_count,
        timer_runtime_diag.legacy_driver_branch_count,
        timer_runtime_diag.default_branch_count,
        timer_runtime_diag.create_count,
        timer_runtime_diag.create_from_ensure_count,
        timer_runtime_diag.create_from_wake_count,
        timer_runtime_diag.create_from_enqueue_count,
        timer_runtime_diag.create_last_mode,
        timer_runtime_diag.create_last_source,
        timer_runtime_diag.create_last_ptr,
        timer_runtime_diag.process_skip_inactive_count,
        timer_runtime_diag.process_skip_not_due_count,
        timer_runtime_diag.process_last_skip_callback_ptr,
        timer_runtime_diag.process_last_skip_arg_ptr,
        timer_runtime_diag.process_last_skip_now_us,
        timer_runtime_diag.process_last_skip_due_us,
        timer_runtime_diag.mark_ready_count,
        timer_runtime_diag.pop_count,
        timer_runtime_diag.selected_count,
        timer_runtime_diag.sleep_count,
        timer_runtime_diag.sleep_true_count,
        timer_runtime_diag.sleep_false_count,
        timer_runtime_diag.sleep_last_task_ptr,
        timer_runtime_diag.sleep_last_wake_at_us,
        timer_runtime_diag.sleep_last_result as u8,
        timer_runtime_diag.sleep_task_mismatch_count,
        timer_runtime_diag.task_ptr,
        timer_runtime_diag.legacy_compat_enabled,
    );
    let scheduler_timer_wake_diag = esp_radio::diagnostic_scheduler_timer_wake_diag();
    println!(
        "upload_http: boot_scan_only_diag scheduler_timer_wake_diag after={} schedule_call_count={} schedule_accept_count={} schedule_past_count={} schedule_infinite_count={} schedule_last_task_ptr=0x{:x} schedule_last_wake_at_us={} tick_count={} handle_alarm_call_count={} handle_alarm_skip_count={} handle_alarm_process_count={} ready_count={} last_ready_task_ptr=0x{:x} last_now_us={} last_current_alarm_us={} last_queue_next_wakeup_us={}",
        stage,
        scheduler_timer_wake_diag.schedule_call_count,
        scheduler_timer_wake_diag.schedule_accept_count,
        scheduler_timer_wake_diag.schedule_past_count,
        scheduler_timer_wake_diag.schedule_infinite_count,
        scheduler_timer_wake_diag.schedule_last_task_ptr,
        scheduler_timer_wake_diag.schedule_last_wake_at_us,
        scheduler_timer_wake_diag.tick_count,
        scheduler_timer_wake_diag.handle_alarm_call_count,
        scheduler_timer_wake_diag.handle_alarm_skip_count,
        scheduler_timer_wake_diag.handle_alarm_process_count,
        scheduler_timer_wake_diag.ready_count,
        scheduler_timer_wake_diag.last_ready_task_ptr,
        scheduler_timer_wake_diag.last_now_us,
        scheduler_timer_wake_diag.last_current_alarm_us,
        scheduler_timer_wake_diag.last_queue_next_wakeup_us,
    );
    let legacy_bootstrap_shim_diag = esp_radio::diagnostic_legacy_bootstrap_shim_diag();
    println!(
        "upload_http: boot_scan_only_diag legacy_bootstrap_shim after={} ran={} scheduler_initialized={} timer_task_precreated={} timer_task_started={} yielded_once={} call_count={}",
        stage,
        legacy_bootstrap_shim_diag.ran as u8,
        legacy_bootstrap_shim_diag.scheduler_initialized as u8,
        legacy_bootstrap_shim_diag.timer_task_precreated as u8,
        legacy_bootstrap_shim_diag.timer_task_started as u8,
        legacy_bootstrap_shim_diag.yielded_once as u8,
        legacy_bootstrap_shim_diag.call_count,
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
                "upload_http: boot_scan_only_diag timer_compat_setfn_recent after={} idx={} ordinal={} ets_timer_ptr=0x{:x} timer_handle_ptr=0x{:x} callback_ptr=0x{:x} arg_ptr=0x{:x} caller_ptr=0x{:x}",
                stage,
                idx,
                ordinal,
                timer_diag.recent_setfn_ets_timer_ptrs[idx],
                timer_diag.recent_setfn_timer_handle_ptrs[idx],
                timer_diag.recent_setfn_callback_ptrs[idx],
                timer_diag.recent_setfn_arg_ptrs[idx],
                timer_diag.recent_setfn_caller_ptrs[idx],
            );
        }
    }
    for idx in 0..timer_diag.recent_exec_ordinals.len() {
        let ordinal = timer_diag.recent_exec_ordinals[idx];
        if ordinal != 0 {
            println!(
                "upload_http: boot_scan_only_diag timer_compat_exec_recent after={} idx={} ordinal={} callback_ptr=0x{:x} arg_ptr=0x{:x} pre_op_chan=0x{:02x} pre_scan_word00=0x{:08x} pre_scan_word114=0x{:08x}",
                stage,
                idx,
                ordinal,
                timer_diag.recent_exec_callback_ptrs[idx],
                timer_diag.recent_exec_arg_ptrs[idx],
                timer_diag.recent_exec_op_chans[idx],
                timer_diag.recent_exec_scan_word00[idx],
                timer_diag.recent_exec_scan_word114[idx],
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
    log_legacy_preempt_builtin(stage);
}
