use super::blob_state_diag::log_blob_state_diag;
use super::boot_scan_idf_compare::{
    maybe_run_boot_scan_only_idf_explicit_compare, run_boot_scan_only_idf_null_compare,
};
use super::maybe_run_boot_scan_only_promisc_diag;
use super::*;

use core::sync::atomic::{AtomicBool, Ordering};
use esp_wifi_sys::c_types::{c_char, c_void};

const WIFI_BOOT_SCAN_ONLY_DIAG: bool =
    parse_nonzero_flag(match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG") {
        Some(value) => Some(value),
        None => option_env!("WIFI_BOOT_SCAN_ONLY_DIAG"),
    });
const WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST") {
        Some(value) => Some(value),
        None => option_env!("WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST"),
    },
);
const WIFI_BOOT_SCAN_ONLY_FORCE_WAKEUP_DIAG: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_FORCE_WAKEUP_DIAG") {
        Some(value) => Some(value),
        None => option_env!("WIFI_BOOT_SCAN_ONLY_FORCE_WAKEUP_DIAG"),
    },
);
const WIFI_BOOT_SCAN_ONLY_FORCE_PHY_ENABLE_DIAG: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_FORCE_PHY_ENABLE_DIAG") {
        Some(value) => Some(value),
        None => option_env!("WIFI_BOOT_SCAN_ONLY_FORCE_PHY_ENABLE_DIAG"),
    },
);
const WIFI_BOOT_SCAN_ONLY_DIAG_SCAN_TIMEOUT_MS: u64 = 15_000;
static WIFI_BOOT_SCAN_ONLY_DIAG_RAN: AtomicBool = AtomicBool::new(false);

unsafe extern "C" {
    fn esp_rtos_task_role(task: *const c_void) -> *const c_char;
}

fn format_task_role(task_ptr: usize) -> &'static str {
    if task_ptr == 0 {
        return "<none>";
    }
    let role_ptr = unsafe { esp_rtos_task_role(task_ptr as *const c_void) };
    if role_ptr.is_null() {
        return "<null>";
    }
    unsafe { core::str::from_utf8_unchecked(core::ffi::CStr::from_ptr(role_ptr.cast()).to_bytes()) }
}

fn log_boot_scan_only_diag_counters(stage: &str) {
    log_blob_state_diag(stage);
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
        "upload_http: boot_scan_only_diag wifi_os_diag after={} sem_take={} sem_take_isr={} sem_give={} sem_give_isr={} queue_send={} queue_send_first_task_ptr=0x{:x} queue_send_first_task_role={} queue_send_last_task_ptr=0x{:x} queue_send_last_task_role={} queue_send_task_changes={} queue_send_last_item_size={} queue_send_last_item_word0=0x{:08x} queue_send_last_item_word1=0x{:08x} queue_send_last_item_pointee_word0=0x{:08x} queue_send_last_item_pointee_word1=0x{:08x} queue_send_last_timer_callback_ptr=0x{:x} queue_send_last_timer_arg_ptr=0x{:x} queue_send_isr={} queue_recv={} queue_recv_first_task_ptr=0x{:x} queue_recv_first_task_role={} queue_recv_last_task_ptr=0x{:x} queue_recv_last_task_role={} queue_recv_task_changes={} queue_recv_isr={} event_post={}",
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
        os_diag.queue_send_last_timer_callback_ptr,
        os_diag.queue_send_last_timer_arg_ptr,
        os_diag.queue_send_isr,
        os_diag.queue_recv,
        os_diag.queue_recv_first_task_ptr,
        format_task_role(os_diag.queue_recv_first_task_ptr),
        os_diag.queue_recv_last_task_ptr,
        format_task_role(os_diag.queue_recv_last_task_ptr),
        os_diag.queue_recv_task_changes,
        os_diag.queue_recv_isr,
        os_diag.event_post,
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
                "upload_http: boot_scan_only_diag wifi_os_diag_send_recent after={} idx={} ordinal={} queue_ptr=0x{:x} task_ptr=0x{:x} task_role={} item_word0=0x{:08x} pointee_word0=0x{:08x} pointee_word1=0x{:08x} timer_callback_ptr=0x{:x} timer_arg_ptr=0x{:x}",
                stage,
                idx,
                ordinal,
                queue_ptr,
                task_ptr,
                format_task_role(task_ptr),
                os_diag.queue_send_recent_item_word0[idx],
                os_diag.queue_send_recent_item_pointee_word0[idx],
                os_diag.queue_send_recent_item_pointee_word1[idx],
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
                "upload_http: boot_scan_only_diag wifi_os_diag_recv_sample after={} idx={} queue_ptr=0x{:x} task_ptr=0x{:x} task_role={}",
                stage,
                idx,
                queue_ptr,
                task_ptr,
                format_task_role(task_ptr),
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
    let timer_diag = esp_radio::diagnostic_timer_compat_diag();
    println!(
        "upload_http: boot_scan_only_diag timer_compat_diag after={} setfn_count={} arm_count={} wrapper_arm_count={} last_ets_timer_ptr=0x{:x} last_timer_handle_ptr=0x{:x} last_callback_ptr=0x{:x} last_arg_ptr=0x{:x} last_arm_us={} last_arm_repeat={} suppressed_setfn_count={} last_suppressed_setfn_callback_ptr=0x{:x} last_suppressed_setfn_arg_ptr=0x{:x} suppressed_arm_count={} last_suppressed_callback_ptr=0x{:x} last_suppressed_arg_ptr=0x{:x} last_suppressed_us={}",
        stage,
        timer_diag.setfn_count,
        timer_diag.arm_count,
        timer_diag.wrapper_arm_count,
        timer_diag.last_ets_timer_ptr,
        timer_diag.last_timer_handle_ptr,
        timer_diag.last_callback_ptr,
        timer_diag.last_arg_ptr,
        timer_diag.last_arm_us,
        timer_diag.last_arm_repeat,
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
}

fn maybe_acquire_boot_scan_only_force_wakeup() -> bool {
    if !WIFI_BOOT_SCAN_ONLY_FORCE_WAKEUP_DIAG {
        return false;
    }
    let rc = unsafe { esp_wifi_sys::include::esp_wifi_force_wakeup_acquire() };
    println!(
        "upload_http: boot_scan_only_diag force_wakeup_acquire rc={}",
        rc
    );
    rc == 0
}

fn maybe_release_boot_scan_only_force_wakeup(acquired: bool) {
    if !acquired {
        return;
    }
    let rc = unsafe { esp_wifi_sys::include::esp_wifi_force_wakeup_release() };
    println!(
        "upload_http: boot_scan_only_diag force_wakeup_release rc={}",
        rc
    );
}

fn maybe_acquire_boot_scan_only_force_phy() -> bool {
    if !WIFI_BOOT_SCAN_ONLY_FORCE_PHY_ENABLE_DIAG {
        return false;
    }
    unsafe { esp_radio::diagnostic_wifi_phy_enable() };
    println!("upload_http: boot_scan_only_diag force_phy_enable invoked=true");
    true
}

fn maybe_release_boot_scan_only_force_phy(acquired: bool) {
    if !acquired {
        return;
    }
    unsafe { esp_radio::diagnostic_wifi_phy_disable() };
    println!("upload_http: boot_scan_only_diag force_phy_disable invoked=true");
}

pub(super) async fn maybe_run_boot_scan_only_diag(
    controller: &mut WifiController<'static>,
    credentials_present: bool,
) {
    if !WIFI_BOOT_SCAN_ONLY_DIAG || credentials_present {
        return;
    }
    if WIFI_BOOT_SCAN_ONLY_DIAG_RAN.swap(true, Ordering::Relaxed) {
        return;
    }

    esp_radio::diagnostic_reset_wifi_mac_isr_count();
    esp_radio::diagnostic_wifi_os_diag_reset();
    esp_radio::diagnostic_reset_phy_common_clock_diag();
    esp_radio::wifi::diagnostic_reset_wifi_rx_cb_counts();
    esp_radio::diagnostic_reset_wifi_scan_done_eventpost_diag();
    esp_radio::diagnostic_reset_timer_compat_diag();
    esp_radio::diagnostic_reset_timer_callback_exec_diag();
    println!("upload_http: boot_scan_only_diag begin credentials_present=false");

    if let Err(err) = wifi_set_mode(controller, wifi_sta_mode()) {
        println!(
            "upload_http: boot_scan_only_diag outcome=set_mode_err err={:?}",
            err
        );
        return;
    }

    match with_timeout(
        Duration::from_millis(WIFI_DRIVER_CONTROL_TIMEOUT_MS),
        wifi_start_async(controller),
    )
    .await
    {
        Ok(Ok(())) => {
            println!("upload_http: boot_scan_only_diag start=ok");
        }
        Ok(Err(err)) => {
            println!(
                "upload_http: boot_scan_only_diag outcome=start_err err={:?}",
                err
            );
            return;
        }
        Err(_) => {
            println!(
                "upload_http: boot_scan_only_diag outcome=start_timeout timeout_ms={}",
                WIFI_DRIVER_CONTROL_TIMEOUT_MS
            );
            return;
        }
    }

    log_boot_scan_only_driver_state();
    let force_wakeup_acquired = maybe_acquire_boot_scan_only_force_wakeup();
    let force_phy_acquired = maybe_acquire_boot_scan_only_force_phy();
    maybe_run_boot_scan_only_promisc_diag().await;

    if WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST {
        println!("upload_http: boot_scan_only_diag idf_null_first begin=true");
        run_boot_scan_only_idf_null_compare();
        log_boot_scan_only_diag_counters("idf_compare_first");
        if maybe_run_boot_scan_only_idf_explicit_compare() {
            log_boot_scan_only_diag_counters("idf_explicit_compare_first");
        }
    }

    let scan_started_at = Instant::now();
    let scan_config = driver::raw_broad_scan_config().with_max(WIFI_SCAN_DIAG_MAX_APS);
    match with_timeout(
        Duration::from_millis(WIFI_BOOT_SCAN_ONLY_DIAG_SCAN_TIMEOUT_MS),
        wifi_scan_with_config_async(controller, scan_config),
    )
    .await
    {
        Ok(Ok(results)) => {
            println!(
                "upload_http: boot_scan_only_diag scan=ok elapsed_ms={} result_count={}",
                elapsed_ms_u32(scan_started_at),
                results.len(),
            );
            log_boot_scan_only_diag_counters("rust_scan");
            for (idx, ap) in results.iter().take(10).enumerate() {
                println!(
                    "upload_http: boot_scan_only_diag ap idx={} ssid={} channel={} bssid={} rssi={} auth={:?}",
                    idx,
                    ap.ssid,
                    ap.channel,
                    format_bssid(ap.bssid),
                    ap.signal_strength,
                    ap.auth_method,
                );
            }
        }
        Ok(Err(err)) => {
            println!(
                "upload_http: boot_scan_only_diag outcome=scan_err elapsed_ms={} err={:?}",
                elapsed_ms_u32(scan_started_at),
                err
            );
        }
        Err(_) => {
            println!(
                "upload_http: boot_scan_only_diag outcome=scan_timeout timeout_ms={}",
                WIFI_BOOT_SCAN_ONLY_DIAG_SCAN_TIMEOUT_MS
            );
        }
    }

    if !WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST {
        run_boot_scan_only_idf_null_compare();
        log_boot_scan_only_diag_counters("idf_compare");
        if maybe_run_boot_scan_only_idf_explicit_compare() {
            log_boot_scan_only_diag_counters("idf_explicit_compare");
        }
    }

    maybe_release_boot_scan_only_force_phy(force_phy_acquired);
    maybe_release_boot_scan_only_force_wakeup(force_wakeup_acquired);

    match with_timeout(
        Duration::from_millis(WIFI_DRIVER_CONTROL_TIMEOUT_MS),
        wifi_stop_async(controller),
    )
    .await
    {
        Ok(Ok(())) => {
            println!("upload_http: boot_scan_only_diag stop=ok");
        }
        Ok(Err(err)) => {
            println!(
                "upload_http: boot_scan_only_diag outcome=stop_err err={:?}",
                err
            );
        }
        Err(_) => {
            println!(
                "upload_http: boot_scan_only_diag outcome=stop_timeout timeout_ms={}",
                WIFI_DRIVER_CONTROL_TIMEOUT_MS
            );
        }
    }
}
