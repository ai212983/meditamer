use super::legacy_port_runtime_enabled;
use esp_println::println;

unsafe extern "C" {
    fn __esp_rtos_diag_task_create_count() -> u32;
    fn __esp_rtos_diag_task_create_last_requested_priority() -> u32;
    fn __esp_rtos_diag_task_create_last_effective_priority() -> u32;
    fn __esp_rtos_diag_legacy_task_model_entry_count() -> usize;
    fn __esp_rtos_diag_queue_create_count() -> u32;
    fn __esp_rtos_diag_wifi_task_selected_count() -> u32;
}

pub(crate) fn log_runtime_state(stage: &str) {
    if !legacy_port_runtime_enabled() {
        return;
    }

    let os_diag = esp_radio::diagnostic_wifi_os_diag_snapshot();
    let adapter_diag = esp_radio::diagnostic_wifi_adapter_primitive_diag();
    let scan_done = esp_radio::diagnostic_wifi_scan_done_eventpost_diag();
    let task_create = esp_radio::diagnostic_wifi_task_create_diag();
    let init_config = esp_radio::diagnostic_wifi_init_config_diag();
    let common_legacy = esp_radio::diagnostic_common_legacy_literal_diag();
    let common_queue = esp_radio::diagnostic_common_legacy_queue_diag();
    let internal_event = esp_radio::diagnostic_internal_legacy_event_post_diag();
    let preempt_legacy = esp_radio::diagnostic_preempt_legacy_backend_diag();
    let legacy_builtin = esp_radio::diagnostic_legacy_builtin_scheduler_diag();
    let legacy_preempt = esp_radio::diagnostic_legacy_preempt_builtin_diag();
    let rtos_task_create_count = unsafe { __esp_rtos_diag_task_create_count() };
    let rtos_task_create_last_requested_priority =
        unsafe { __esp_rtos_diag_task_create_last_requested_priority() };
    let rtos_task_create_last_effective_priority =
        unsafe { __esp_rtos_diag_task_create_last_effective_priority() };
    let rtos_legacy_task_model_entry_count =
        unsafe { __esp_rtos_diag_legacy_task_model_entry_count() };
    let rtos_queue_create_count = unsafe { __esp_rtos_diag_queue_create_count() };
    let rtos_wifi_task_selected_count = unsafe { __esp_rtos_diag_wifi_task_selected_count() };

    println!(
        "upload_http: legacy_port runtime_state after={} wifi_mac_isr_count={} queue_send={} queue_send_isr={} queue_recv={} event_post={} thread_sem_get={} task_get_current_task_count={} scan_done_count={} scan_done_ap_num={} legacy_builtin_initialized={} legacy_builtin_switch_count={} legacy_preempt_initialized={} legacy_preempt_current_task=0x{:x} legacy_preempt_thread_sem=0x{:x}",
        stage,
        esp_radio::diagnostic_wifi_mac_isr_count(),
        os_diag.queue_send,
        os_diag.queue_send_isr,
        os_diag.queue_recv,
        os_diag.event_post,
        adapter_diag.thread_sem_get_count,
        adapter_diag.task_get_current_task_count,
        scan_done.count,
        scan_done.ap_num,
        legacy_builtin.initialized as u8,
        legacy_builtin.switch_count,
        legacy_preempt.initialized as u8,
        legacy_preempt.current_task,
        legacy_preempt.current_task_thread_semaphore,
    );
    let mut legacy_slot_count = 0usize;
    for idx in 0..legacy_builtin.task_ptrs.len() {
        let task_ptr = legacy_builtin.task_ptrs[idx];
        if task_ptr != 0 {
            legacy_slot_count += 1;
            let role = core::str::from_utf8(&legacy_builtin.task_roles[idx])
                .ok()
                .map(|s| s.trim_end_matches('\0'))
                .unwrap_or("<invalid>");
            println!(
                "upload_http: legacy_port runtime_state_slot after={} idx={} task_ptr=0x{:x} role={}{}",
                stage,
                idx,
                task_ptr,
                role,
                if task_ptr == legacy_builtin.current_task {
                    " current=true"
                } else if task_ptr == legacy_builtin.last_selected_task {
                    " last_selected=true"
                } else {
                    ""
                },
            );
        }
    }
    println!(
        "upload_http: legacy_port runtime_state_task after={} task_create_count={} recent_name_tag0=0x{:08x} recent_stack0={} recent_prio0={} recent_core0={} recent_task_ptr0=0x{:x} init_config_ptr=0x{:x} osi_funcs_ptr=0x{:x} wifi_task_core_id={} init_magic=0x{:08x} osi_queue_create_ptr=0x{:x} osi_queue_recv_ptr=0x{:x} osi_task_create_ptr=0x{:x} osi_task_create_pinned_ptr=0x{:x} osi_task_get_current_ptr=0x{:x} osi_thread_sem_get_ptr=0x{:x} osi_event_post_ptr=0x{:x} osi_malloc_internal_ptr=0x{:x} rtos_task_create_count={} rtos_task_create_last_requested_priority={} rtos_task_create_last_effective_priority={} rtos_legacy_task_model_entry_count={} rtos_queue_create_count={} rtos_wifi_task_selected_count={} common_task_create_count={} common_task_last_ptr=0x{:x} common_task_last_stack={} common_thread_sem_get_count={} common_thread_sem_last_ptr=0x{:x} common_queue_create_count={} common_queue_last_len={} common_queue_last_item_size={} common_queue_send_count={} common_queue_send_front_count={} common_queue_recv_count={} common_queue_send_isr_count={} common_queue_recv_isr_count={} common_queue_last_ptr=0x{:x} internal_event_post_count={} internal_event_last_id={} internal_scan_done_status={} internal_scan_done_number={} internal_scan_done_id={} internal_scan_done_ap_num_rc={} internal_scan_done_ap_num={} preempt_enable_count={} preempt_yield_count={} preempt_current_task_count={} preempt_current_task_last_ptr=0x{:x} preempt_thread_sem_count={} preempt_thread_sem_last_ptr=0x{:x} preempt_task_create_count={} preempt_task_last_ptr=0x{:x} preempt_task_last_stack={} preempt_schedule_delete_count={} legacy_slot_count={}",
        stage,
        task_create.count,
        task_create.recent_name_tags[0],
        task_create.recent_stack_depths[0],
        task_create.recent_prios[0],
        task_create.recent_core_ids[0],
        task_create.recent_task_ptrs[0],
        init_config.config_ptr,
        init_config.osi_funcs_ptr,
        init_config.wifi_task_core_id,
        init_config.magic as u32,
        init_config.osi_queue_create_ptr,
        init_config.osi_queue_recv_ptr,
        init_config.osi_task_create_ptr,
        init_config.osi_task_create_pinned_ptr,
        init_config.osi_task_get_current_ptr,
        init_config.osi_wifi_thread_semphr_get_ptr,
        init_config.osi_event_post_ptr,
        init_config.osi_malloc_internal_ptr,
        rtos_task_create_count,
        rtos_task_create_last_requested_priority,
        rtos_task_create_last_effective_priority,
        rtos_legacy_task_model_entry_count,
        rtos_queue_create_count,
        rtos_wifi_task_selected_count,
        common_legacy.task_create_count,
        common_legacy.task_create_last_task_ptr,
        common_legacy.task_create_last_stack_depth,
        common_legacy.thread_sem_get_count,
        common_legacy.thread_sem_get_last_ptr,
        common_legacy.queue_create_count,
        common_legacy.queue_create_last_len,
        common_legacy.queue_create_last_item_size,
        common_queue.send_count,
        common_queue.send_front_count,
        common_queue.recv_count,
        common_queue.send_isr_count,
        common_queue.recv_isr_count,
        common_queue.last_queue_ptr,
        internal_event.count,
        internal_event.last_event_id,
        internal_event.scan_done_status,
        internal_event.scan_done_number,
        internal_event.scan_done_id,
        internal_event.scan_done_ap_num_rc,
        internal_event.scan_done_ap_num,
        preempt_legacy.enable_count,
        preempt_legacy.yield_count,
        preempt_legacy.current_task_count,
        preempt_legacy.current_task_last_ptr,
        preempt_legacy.current_task_thread_sem_count,
        preempt_legacy.current_task_thread_sem_last_ptr,
        preempt_legacy.task_create_count,
        preempt_legacy.task_create_last_task_ptr,
        preempt_legacy.task_create_last_stack_size,
        preempt_legacy.schedule_delete_count,
        legacy_slot_count,
    );
    println!(
        "upload_http: legacy_port runtime_state_irq after={} osi_set_intr_ptr=0x{:x} osi_clear_intr_ptr=0x{:x} osi_set_isr_ptr=0x{:x} osi_ints_on_ptr=0x{:x} osi_ints_off_ptr=0x{:x} osi_wifi_int_disable_ptr=0x{:x} osi_wifi_int_restore_ptr=0x{:x} osi_task_yield_from_isr_ptr=0x{:x} osi_queue_send_from_isr_ptr=0x{:x}",
        stage,
        init_config.osi_set_intr_ptr,
        init_config.osi_clear_intr_ptr,
        init_config.osi_set_isr_ptr,
        init_config.osi_ints_on_ptr,
        init_config.osi_ints_off_ptr,
        init_config.osi_wifi_int_disable_ptr,
        init_config.osi_wifi_int_restore_ptr,
        init_config.osi_task_yield_from_isr_ptr,
        init_config.osi_queue_send_from_isr_ptr,
    );
    for idx in 0..common_queue.recent_send_ordinals.len() {
        let ordinal = common_queue.recent_send_ordinals[idx];
        if ordinal != 0 {
            println!(
                "upload_http: legacy_port runtime_state_common_queue_send_recent after={} idx={} ordinal={} item_word0=0x{:08x} pointee_word0=0x{:08x} pointee_word1=0x{:08x}",
                stage,
                idx,
                ordinal,
                common_queue.recent_send_item_word0[idx],
                common_queue.recent_send_item_pointee_word0[idx],
                common_queue.recent_send_item_pointee_word1[idx],
            );
        }
    }
    for idx in 0..common_queue.recent_recv_ordinals.len() {
        let ordinal = common_queue.recent_recv_ordinals[idx];
        if ordinal != 0 {
            println!(
                "upload_http: legacy_port runtime_state_common_queue_recv_recent after={} idx={} ordinal={} item_word0=0x{:08x} pointee_word0=0x{:08x} pointee_word1=0x{:08x}",
                stage,
                idx,
                ordinal,
                common_queue.recent_recv_item_word0[idx],
                common_queue.recent_recv_item_pointee_word0[idx],
                common_queue.recent_recv_item_pointee_word1[idx],
            );
        }
    }
}
