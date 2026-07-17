use super::task_role::format_task_role;
use esp_println::println;

pub(super) fn log_boot_scan_only_core_counters(stage: &str) {
    let sem_trace = esp_rtos::diagnostic_esp_radio_sem_trace_snapshot();
    println!(
        "upload_http: boot_scan_only_diag sem_trace after={} take_wait={} take_done={} give={} waitq_sleep={} waitq_notify={} last_event={} last_task_ptr=0x{:x} last_object_ptr=0x{:x} last_value={}",
        stage,
        sem_trace.take_wait_count,
        sem_trace.take_done_count,
        sem_trace.give_count,
        sem_trace.wait_queue_sleep_count,
        sem_trace.wait_queue_notify_count,
        sem_trace.last_event,
        sem_trace.last_task_ptr,
        sem_trace.last_object_ptr,
        sem_trace.last_value,
    );
    let phy_common_clock = esp_radio::diagnostic_phy_common_clock_diag();
    println!(
        "upload_http: boot_scan_only_diag phy_common_clock after={} enable_calls={} disable_calls={} ref_count={} real_enable={}",
        stage,
        phy_common_clock.enable_calls,
        phy_common_clock.disable_calls,
        phy_common_clock.ref_count,
        phy_common_clock.real_enable as u8,
    );
    println!(
        "upload_http: boot_scan_only_diag wifi_mac_isr_count after={} count={}",
        stage,
        esp_radio::diagnostic_wifi_mac_isr_count()
    );
    let (rx_sta, rx_ap) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    println!(
        "upload_http: boot_scan_only_diag wifi_rx_cb_count after={} sta={} ap={}",
        stage, rx_sta, rx_ap
    );
    let eventpost = esp_radio::diagnostic_wifi_scan_done_eventpost_diag();
    println!(
        "upload_http: boot_scan_only_diag scan_done_eventpost after={} count={} status={} number={} scan_id={} ap_num_rc={} ap_num={}",
        stage,
        eventpost.count,
        eventpost.status,
        eventpost.number,
        eventpost.scan_id,
        eventpost.ap_num_rc,
        eventpost.ap_num,
    );
    let os_diag = esp_radio::diagnostic_wifi_os_diag_snapshot();
    println!(
        "upload_http: boot_scan_only_diag wifi_os_diag after={} sem_take={} sem_take_isr={} sem_give={} sem_give_isr={} queue_send={} queue_send_first_task_ptr=0x{:x} queue_send_first_task_role={} queue_send_last_task_ptr=0x{:x} queue_send_last_task_role={} queue_send_task_changes={} queue_send_last_item_size={} queue_send_last_item_word0=0x{:08x} queue_send_last_item_word1=0x{:08x} queue_send_last_item_pointee_word0=0x{:08x} queue_send_last_item_pointee_word1=0x{:08x} queue_send_last_caller_ptr=0x{:x} queue_send_last_timer_callback_ptr=0x{:x} queue_send_last_timer_arg_ptr=0x{:x} queue_send_isr={} queue_send_isr_legacy_branch={} queue_send_scan_start_process_count={} queue_send_get_ap_list_process_count={} queue_send_clear_ap_list_process_count={} queue_send_set_promis_process_count={} queue_recv={} queue_recv_first_task_ptr=0x{:x} queue_recv_first_task_role={} queue_recv_last_task_ptr=0x{:x} queue_recv_last_task_role={} queue_recv_task_changes={} queue_recv_isr={} queue_recv_last_item_size={} queue_recv_last_item_word0=0x{:08x} queue_recv_last_item_word1=0x{:08x} queue_recv_last_item_pointee_word0=0x{:08x} queue_recv_last_item_pointee_word1=0x{:08x} queue_recv_last_caller_ptr=0x{:x} queue_recv_scan_start_process_count={} queue_recv_get_ap_list_process_count={} queue_recv_clear_ap_list_process_count={} queue_recv_set_promis_process_count={} event_post={} wifi_log_callback_count={}",
        stage,
        os_diag.sem_take,
        os_diag.sem_take_isr,
        os_diag.sem_give,
        os_diag.sem_give_isr,
        os_diag.queue_send,
        os_diag.queue_send_first_task_ptr,
        format_task_role(os_diag.queue_send_first_task_ptr),
        os_diag.queue_send_last_task_ptr,
        format_task_role(os_diag.queue_send_last_task_ptr),
        os_diag.queue_send_task_changes,
        os_diag.queue_send_last_item_size,
        os_diag.queue_send_last_item_word0,
        os_diag.queue_send_last_item_word1,
        os_diag.queue_send_last_item_pointee_word0,
        os_diag.queue_send_last_item_pointee_word1,
        os_diag.queue_send_last_caller_ptr,
        os_diag.queue_send_last_timer_callback_ptr,
        os_diag.queue_send_last_timer_arg_ptr,
        os_diag.queue_send_isr,
        os_diag.queue_send_isr_legacy_branch,
        os_diag.queue_send_scan_start_process_count,
        os_diag.queue_send_get_ap_list_process_count,
        os_diag.queue_send_clear_ap_list_process_count,
        os_diag.queue_send_set_promis_process_count,
        os_diag.queue_recv,
        os_diag.queue_recv_first_task_ptr,
        format_task_role(os_diag.queue_recv_first_task_ptr),
        os_diag.queue_recv_last_task_ptr,
        format_task_role(os_diag.queue_recv_last_task_ptr),
        os_diag.queue_recv_task_changes,
        os_diag.queue_recv_isr,
        os_diag.queue_recv_last_item_size,
        os_diag.queue_recv_last_item_word0,
        os_diag.queue_recv_last_item_word1,
        os_diag.queue_recv_last_item_pointee_word0,
        os_diag.queue_recv_last_item_pointee_word1,
        os_diag.queue_recv_last_caller_ptr,
        os_diag.queue_recv_scan_start_process_count,
        os_diag.queue_recv_get_ap_list_process_count,
        os_diag.queue_recv_clear_ap_list_process_count,
        os_diag.queue_recv_set_promis_process_count,
        os_diag.event_post,
        os_diag.wifi_log_callback_count,
    );
    println!(
        "upload_http: boot_scan_only_diag wifi_os_diag_init_processes after={} queue_send_set_rxcb_process_count={} queue_send_register_mgmt_frame_process_count={} queue_send_set_country_process_count={} queue_send_set_ps_process_count={} queue_recv_set_rxcb_process_count={} queue_recv_register_mgmt_frame_process_count={} queue_recv_set_country_process_count={} queue_recv_set_ps_process_count={}",
        stage,
        os_diag.queue_send_set_rxcb_process_count,
        os_diag.queue_send_register_mgmt_frame_process_count,
        os_diag.queue_send_set_country_process_count,
        os_diag.queue_send_set_ps_process_count,
        os_diag.queue_recv_set_rxcb_process_count,
        os_diag.queue_recv_register_mgmt_frame_process_count,
        os_diag.queue_recv_set_country_process_count,
        os_diag.queue_recv_set_ps_process_count,
    );
    for idx in 0..os_diag.queue_send_sample_queues.len() {
        let queue_ptr = os_diag.queue_send_sample_queues[idx];
        let task_ptr = os_diag.queue_send_sample_tasks[idx];
        if queue_ptr != 0 || task_ptr != 0 {
            println!(
                "upload_http: boot_scan_only_diag wifi_os_diag_send_sample after={} idx={} queue_ptr=0x{:x} task_ptr=0x{:x} task_role={} item_word0=0x{:08x} pointee_word0=0x{:08x} pointee_word1=0x{:08x}",
                stage,
                idx,
                queue_ptr,
                task_ptr,
                format_task_role(task_ptr),
                os_diag.queue_send_sample_item_word0[idx],
                os_diag.queue_send_sample_item_pointee_word0[idx],
                os_diag.queue_send_sample_item_pointee_word1[idx],
            );
            println!(
                "upload_http: boot_scan_only_diag wifi_os_diag_send_sample_timer after={} idx={} timer_callback_ptr=0x{:x} timer_arg_ptr=0x{:x}",
                stage,
                idx,
                os_diag.queue_send_sample_timer_callback_ptr[idx],
                os_diag.queue_send_sample_timer_arg_ptr[idx],
            );
        }
    }
    for idx in 0..os_diag.queue_send_recent_ordinals.len() {
        let ordinal = os_diag.queue_send_recent_ordinals[idx];
        let queue_ptr = os_diag.queue_send_recent_queues[idx];
        let task_ptr = os_diag.queue_send_recent_tasks[idx];
        if ordinal != 0 || queue_ptr != 0 || task_ptr != 0 {
            println!(
                "upload_http: boot_scan_only_diag wifi_os_diag_send_recent after={} idx={} ordinal={} queue_ptr=0x{:x} task_ptr=0x{:x} task_role={} item_word0=0x{:08x} pointee_word0=0x{:08x} pointee_word1=0x{:08x} caller_ptr=0x{:x} timer_callback_ptr=0x{:x} timer_arg_ptr=0x{:x}",
                stage,
                idx,
                ordinal,
                queue_ptr,
                task_ptr,
                format_task_role(task_ptr),
                os_diag.queue_send_recent_item_word0[idx],
                os_diag.queue_send_recent_item_pointee_word0[idx],
                os_diag.queue_send_recent_item_pointee_word1[idx],
                os_diag.queue_send_recent_caller_ptr[idx],
                os_diag.queue_send_recent_timer_callback_ptr[idx],
                os_diag.queue_send_recent_timer_arg_ptr[idx],
            );
        }
    }
    for idx in 0..os_diag.queue_recv_sample_queues.len() {
        let queue_ptr = os_diag.queue_recv_sample_queues[idx];
        let task_ptr = os_diag.queue_recv_sample_tasks[idx];
        if queue_ptr != 0 || task_ptr != 0 {
            println!(
                "upload_http: boot_scan_only_diag wifi_os_diag_recv_sample after={} idx={} queue_ptr=0x{:x} task_ptr=0x{:x} task_role={} item_word0=0x{:08x} pointee_word0=0x{:08x} pointee_word1=0x{:08x}",
                stage,
                idx,
                queue_ptr,
                task_ptr,
                format_task_role(task_ptr),
                os_diag.queue_recv_sample_item_word0[idx],
                os_diag.queue_recv_sample_item_pointee_word0[idx],
                os_diag.queue_recv_sample_item_pointee_word1[idx],
            );
        }
    }
    for idx in 0..os_diag.queue_recv_recent_ordinals.len() {
        let ordinal = os_diag.queue_recv_recent_ordinals[idx];
        let queue_ptr = os_diag.queue_recv_recent_queues[idx];
        let task_ptr = os_diag.queue_recv_recent_tasks[idx];
        if ordinal != 0 || queue_ptr != 0 || task_ptr != 0 {
            println!(
                "upload_http: boot_scan_only_diag wifi_os_diag_recv_recent after={} idx={} ordinal={} queue_ptr=0x{:x} task_ptr=0x{:x} task_role={} item_word0=0x{:08x} pointee_word0=0x{:08x} pointee_word1=0x{:08x} caller_ptr=0x{:x}",
                stage,
                idx,
                ordinal,
                queue_ptr,
                task_ptr,
                format_task_role(task_ptr),
                os_diag.queue_recv_recent_item_word0[idx],
                os_diag.queue_recv_recent_item_pointee_word0[idx],
                os_diag.queue_recv_recent_item_pointee_word1[idx],
                os_diag.queue_recv_recent_caller_ptr[idx],
            );
        }
    }
    for idx in 0..os_diag.wifi_log_recent_ordinals.len() {
        let ordinal = os_diag.wifi_log_recent_ordinals[idx];
        let caller_ptr = os_diag.wifi_log_recent_caller_ptr[idx];
        if ordinal != 0 || caller_ptr != 0 {
            println!(
                "upload_http: boot_scan_only_diag wifi_log_recent after={} idx={} ordinal={} caller_ptr=0x{:x}",
                stage, idx, ordinal, caller_ptr
            );
        }
    }
    let adapter_diag = esp_radio::diagnostic_wifi_adapter_primitive_diag();
    println!(
        "upload_http: boot_scan_only_diag wifi_adapter_diag after={} thread_sem_get={} thread_sem_first_ptr=0x{:x} thread_sem_last_ptr=0x{:x} thread_sem_ptr_changes={} thread_sem_first_task_ptr=0x{:x} thread_sem_first_task_role={} thread_sem_last_task_ptr=0x{:x} thread_sem_last_task_role={} thread_sem_task_changes={} task_delay_count={} task_delay_max_tick={} task_ms_to_tick_count={} task_ms_to_tick_max_ms={} task_get_current_task_count={} task_get_current_task_first_ptr=0x{:x} task_get_current_task_last_ptr=0x{:x} task_get_current_task_changes={} event_group_create={} event_group_set_bits={} event_group_clear_bits={} event_group_wait_bits={} phy_enable_count={} phy_disable_count={}",
        stage,
        adapter_diag.thread_sem_get_count,
        adapter_diag.thread_sem_first_ptr,
        adapter_diag.thread_sem_last_ptr,
        adapter_diag.thread_sem_ptr_change_count,
        adapter_diag.thread_sem_first_task_ptr,
        format_task_role(adapter_diag.thread_sem_first_task_ptr),
        adapter_diag.thread_sem_last_task_ptr,
        format_task_role(adapter_diag.thread_sem_last_task_ptr),
        adapter_diag.thread_sem_task_change_count,
        adapter_diag.task_delay_count,
        adapter_diag.task_delay_max_tick,
        adapter_diag.task_ms_to_tick_count,
        adapter_diag.task_ms_to_tick_max_ms,
        adapter_diag.task_get_current_task_count,
        adapter_diag.task_get_current_task_first_ptr,
        adapter_diag.task_get_current_task_last_ptr,
        adapter_diag.task_get_current_task_change_count,
        adapter_diag.event_group_create_count,
        adapter_diag.event_group_set_bits_count,
        adapter_diag.event_group_clear_bits_count,
        adapter_diag.event_group_wait_bits_count,
        adapter_diag.phy_enable_count,
        adapter_diag.phy_disable_count,
    );
    println!(
        "upload_http: boot_scan_only_diag wifi_alloc_diag after={} malloc_count={} malloc_total={} malloc_max={} malloc_last={} malloc_internal_count={} malloc_internal_total={} malloc_internal_max={} malloc_internal_last={} calloc_internal_count={} calloc_internal_total={} calloc_internal_max={} calloc_internal_last={} wifi_malloc_count={} wifi_malloc_total={} wifi_malloc_max={} wifi_malloc_last={} wifi_calloc_count={} wifi_calloc_total={} wifi_calloc_max={} wifi_calloc_last={} free_count={} free_last_ptr=0x{:x}",
        stage,
        adapter_diag.malloc_count,
        adapter_diag.malloc_total_size,
        adapter_diag.malloc_max_size,
        adapter_diag.malloc_last_size,
        adapter_diag.malloc_internal_count,
        adapter_diag.malloc_internal_total_size,
        adapter_diag.malloc_internal_max_size,
        adapter_diag.malloc_internal_last_size,
        adapter_diag.calloc_internal_count,
        adapter_diag.calloc_internal_total_size,
        adapter_diag.calloc_internal_max_size,
        adapter_diag.calloc_internal_last_size,
        adapter_diag.wifi_malloc_count,
        adapter_diag.wifi_malloc_total_size,
        adapter_diag.wifi_malloc_max_size,
        adapter_diag.wifi_malloc_last_size,
        adapter_diag.wifi_calloc_count,
        adapter_diag.wifi_calloc_total_size,
        adapter_diag.wifi_calloc_max_size,
        adapter_diag.wifi_calloc_last_size,
        adapter_diag.free_count,
        adapter_diag.free_last_ptr,
    );
}
