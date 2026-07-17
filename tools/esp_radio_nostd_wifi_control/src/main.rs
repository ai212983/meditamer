#![no_std]
#![no_main]

mod blob_state;
mod promisc_diag;
mod timer_diag;

use core::ffi::{c_char, c_void};

use esp_backtrace as _;
use esp_hal::timer::timg::TimerGroup;
use esp_println::println;
use esp_radio::wifi::{ScanConfig, WifiMode};
use timer_diag::print_timer_compat_diag;

fn legacy_timer_compat_requested() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_TIMER_COMPAT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RADIO_USE_LEGACY_TIMER_COMPAT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn legacy_timer_compat_init_tasks_requested() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_TIMER_COMPAT_INIT_TASKS_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RADIO_USE_LEGACY_TIMER_COMPAT_INIT_TASKS_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn minimal_log_requested() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_STANDALONE_MINIMAL_LOG_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_STANDALONE_MINIMAL_LOG_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

unsafe extern "C" {
    fn __esp_rtos_diag_reset_legacy_task_model();
    fn __esp_rtos_diag_task_create_count() -> u32;
    fn __esp_rtos_diag_task_create_last_requested_priority() -> u32;
    fn __esp_rtos_diag_task_create_last_effective_priority() -> u32;
    fn __esp_rtos_diag_legacy_task_model_entry_count() -> usize;
    fn __esp_rtos_diag_legacy_task_model_current_index() -> usize;
    fn __esp_rtos_diag_queue_create_count() -> u32;
    fn __esp_rtos_diag_queue_create_last_capacity() -> u32;
    fn __esp_rtos_diag_queue_create_last_item_size() -> u32;
    fn __esp_rtos_diag_wifi_task_selected_count() -> u32;
    fn esp_rtos_task_role(task: *const c_void) -> *const c_char;
}

fn diag_yield(label: &str, count: usize) {
    for _ in 0..count {
        esp_rtos::yield_for_esp_radio_diag();
    }
    println!("nostd_wifi_control: diag_yield label={} count={}", label, count);
}

fn print_rtos_create_diag(label: &str) {
    let task_create_count = unsafe { __esp_rtos_diag_task_create_count() };
    let task_create_last_requested_priority =
        unsafe { __esp_rtos_diag_task_create_last_requested_priority() };
    let task_create_last_effective_priority =
        unsafe { __esp_rtos_diag_task_create_last_effective_priority() };
    let legacy_task_model_entry_count = unsafe { __esp_rtos_diag_legacy_task_model_entry_count() };
    let legacy_task_model_current_index =
        unsafe { __esp_rtos_diag_legacy_task_model_current_index() };
    let queue_create_count = unsafe { __esp_rtos_diag_queue_create_count() };
    let queue_last_capacity = unsafe { __esp_rtos_diag_queue_create_last_capacity() };
    let queue_last_item_size = unsafe { __esp_rtos_diag_queue_create_last_item_size() };
    let wifi_task_selected_count = unsafe { __esp_rtos_diag_wifi_task_selected_count() };
    println!(
        "nostd_wifi_control: rtos_create_diag label={} task_create_count={} task_create_last_requested_priority={} task_create_last_effective_priority={} legacy_task_model_entry_count={} legacy_task_model_current_index={} queue_create_count={} queue_last_capacity={} queue_last_item_size={} wifi_task_selected_count={}",
        label,
        task_create_count,
        task_create_last_requested_priority,
        task_create_last_effective_priority,
        legacy_task_model_entry_count,
        legacy_task_model_current_index,
        queue_create_count,
        queue_last_capacity,
        queue_last_item_size,
        wifi_task_selected_count,
    );
}

fn print_wifi_mac_isr_diag(label: &str) {
    println!(
        "nostd_wifi_control: wifi_mac_isr_diag label={} count={}",
        label,
        esp_radio::diagnostic_wifi_mac_isr_count(),
    );
}

fn print_wifi_promisc_cb_diag(label: &str) {
    println!(
        "nostd_wifi_control: wifi_promisc_cb_diag label={} count={}",
        label,
        esp_radio::diagnostic_wifi_promisc_rx_cb_count(),
    );
}

fn print_wifi_rx_cb_diag(label: &str) {
    let (sta, ap) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    println!(
        "nostd_wifi_control: wifi_rx_cb_diag label={} sta={} ap={}",
        label, sta, ap,
    );
}

fn print_wifi_init_config_diag(label: &str) {
    let diag = esp_radio::diagnostic_wifi_init_config_diag();
    let bootstrap = esp_radio::diagnostic_legacy_bootstrap_shim_diag();
    println!(
        "nostd_wifi_control: wifi_init_config_diag label={} config_ptr=0x{:08x} osi_funcs_ptr=0x{:08x} static_rx_buf_num={} dynamic_rx_buf_num={} static_tx_buf_num={} dynamic_tx_buf_num={} rx_mgmt_buf_type={} rx_mgmt_buf_num={} cache_tx_buf_num={} ampdu_rx_enable={} ampdu_tx_enable={} amsdu_tx_enable={} nvs_enable={} nano_enable={} rx_ba_win={} wifi_task_core_id={} feature_caps=0x{:016x} sta_disconnected_pm={} tx_hetb_queue_num={} dump_hesigb_enable={} magic=0x{:08x}",
        label,
        diag.config_ptr,
        diag.osi_funcs_ptr,
        diag.static_rx_buf_num,
        diag.dynamic_rx_buf_num,
        diag.static_tx_buf_num,
        diag.dynamic_tx_buf_num,
        diag.rx_mgmt_buf_type,
        diag.rx_mgmt_buf_num,
        diag.cache_tx_buf_num,
        diag.ampdu_rx_enable,
        diag.ampdu_tx_enable,
        diag.amsdu_tx_enable,
        diag.nvs_enable,
        diag.nano_enable,
        diag.rx_ba_win,
        diag.wifi_task_core_id,
        diag.feature_caps,
        diag.sta_disconnected_pm,
        diag.tx_hetb_queue_num,
        diag.dump_hesigb_enable,
        diag.magic,
    );
    println!(
        "nostd_wifi_control: legacy_bootstrap_shim_diag label={} ran={} scheduler_initialized={} timer_task_precreated={} timer_task_started={} yielded_once={} call_count={}",
        label,
        bootstrap.ran,
        bootstrap.scheduler_initialized,
        bootstrap.timer_task_precreated,
        bootstrap.timer_task_started,
        bootstrap.yielded_once,
        bootstrap.call_count,
    );
    println!(
        "nostd_wifi_control: wifi_osi_diag label={} set_isr=0x{:08x} queue_create=0x{:08x} queue_recv=0x{:08x} task_create=0x{:08x} task_create_pinned=0x{:08x} task_get_current=0x{:08x} wifi_thread_semphr_get=0x{:08x} timer_arm_us=0x{:08x} event_post=0x{:08x} malloc_internal=0x{:08x}",
        label,
        diag.osi_set_isr_ptr,
        diag.osi_queue_create_ptr,
        diag.osi_queue_recv_ptr,
        diag.osi_task_create_ptr,
        diag.osi_task_create_pinned_ptr,
        diag.osi_task_get_current_ptr,
        diag.osi_wifi_thread_semphr_get_ptr,
        diag.osi_timer_arm_us_ptr,
        diag.osi_event_post_ptr,
        diag.osi_malloc_internal_ptr,
    );
}

fn print_wifi_task_create_diag(label: &str) {
    let diag = esp_radio::diagnostic_wifi_task_create_diag();
    println!(
        "nostd_wifi_control: wifi_task_create_diag label={} count={}",
        label, diag.count,
    );
    for idx in 0..diag.recent_ordinals.len() {
        let ordinal = diag.recent_ordinals[idx];
        if ordinal == 0 {
            continue;
        }
        println!(
            "nostd_wifi_control: wifi_task_create_recent label={} idx={} ordinal={} task_func_ptr=0x{:08x} name_tag=0x{:08x} name_len={} stack_depth={} param_ptr=0x{:08x} prio={} core_id={} task_ptr=0x{:08x}",
            label,
            idx,
            ordinal,
            diag.recent_task_func_ptrs[idx],
            diag.recent_name_tags[idx],
            diag.recent_name_lens[idx],
            diag.recent_stack_depths[idx],
            diag.recent_param_ptrs[idx],
            diag.recent_prios[idx],
            diag.recent_core_ids[idx],
            diag.recent_task_ptrs[idx],
        );
    }
}

fn format_task_role(task_ptr: usize) -> &'static str {
    if task_ptr == 0 {
        return "none";
    }
    let role_ptr = unsafe { esp_rtos_task_role(task_ptr as *const c_void) };
    if role_ptr.is_null() {
        return "unknown";
    }
    let bytes = unsafe { core::slice::from_raw_parts(role_ptr.cast::<u8>(), 24) };
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    match core::str::from_utf8(&bytes[..len]) {
        Ok("") => "unknown",
        Ok("main") => "main",
        Ok("wifi") => "wifi",
        Ok("timer") => "timer",
        Ok("idle") => "idle",
        Ok(_) => "other",
        Err(_) => "invalid",
    }
}

fn print_wifi_os_diag(label: &str) {
    let diag = esp_radio::diagnostic_wifi_os_diag_snapshot();
    let primitive = esp_radio::diagnostic_wifi_adapter_primitive_diag();
    println!(
        "nostd_wifi_control: wifi_os_diag label={} sem_take={} sem_take_isr={} sem_give={} sem_give_isr={} task_yield_from_isr={} queue_send={} queue_send_first_task_ptr=0x{:08x} queue_send_first_task_role={} queue_send_last_task_ptr=0x{:08x} queue_send_last_task_role={} queue_send_isr={} queue_recv={} queue_recv_first_task_ptr=0x{:08x} queue_recv_first_task_role={} queue_recv_last_task_ptr=0x{:08x} queue_recv_last_task_role={} queue_recv_isr={} event_post={} send_task_changes={} recv_task_changes={} send_last_item_size={} send_last_item_word0=0x{:08x} recv_last_item_size={} recv_last_item_word0=0x{:08x} recv_last_caller_ptr=0x{:08x}",
        label,
        diag.sem_take,
        diag.sem_take_isr,
        diag.sem_give,
        diag.sem_give_isr,
        diag.task_yield_from_isr,
        diag.queue_send,
        diag.queue_send_first_task_ptr,
        format_task_role(diag.queue_send_first_task_ptr),
        diag.queue_send_last_task_ptr,
        format_task_role(diag.queue_send_last_task_ptr),
        diag.queue_send_isr,
        diag.queue_recv,
        diag.queue_recv_first_task_ptr,
        format_task_role(diag.queue_recv_first_task_ptr),
        diag.queue_recv_last_task_ptr,
        format_task_role(diag.queue_recv_last_task_ptr),
        diag.queue_recv_isr,
        diag.event_post,
        diag.queue_send_task_changes,
        diag.queue_recv_task_changes,
        diag.queue_send_last_item_size,
        diag.queue_send_last_item_word0,
        diag.queue_recv_last_item_size,
        diag.queue_recv_last_item_word0,
        diag.queue_recv_last_caller_ptr,
    );
    println!(
        "nostd_wifi_control: wifi_adapter_primitive_diag label={} thread_sem_get_count={} thread_sem_first_ptr=0x{:08x} thread_sem_last_ptr=0x{:08x} thread_sem_ptr_change_count={} thread_sem_first_task_ptr=0x{:08x} thread_sem_last_task_ptr=0x{:08x} thread_sem_task_change_count={} task_get_current_task_count={} task_get_current_task_first_ptr=0x{:08x} task_get_current_task_last_ptr=0x{:08x} task_get_current_task_change_count={}",
        label,
        primitive.thread_sem_get_count,
        primitive.thread_sem_first_ptr,
        primitive.thread_sem_last_ptr,
        primitive.thread_sem_ptr_change_count,
        primitive.thread_sem_first_task_ptr,
        primitive.thread_sem_last_task_ptr,
        primitive.thread_sem_task_change_count,
        primitive.task_get_current_task_count,
        primitive.task_get_current_task_first_ptr,
        primitive.task_get_current_task_last_ptr,
        primitive.task_get_current_task_change_count,
    );
    println!(
        "nostd_wifi_control: wifi_alloc_diag label={} malloc_internal_count={} malloc_internal_last={} calloc_internal_count={} calloc_internal_last={} wifi_malloc_count={} wifi_malloc_last={} wifi_calloc_count={} wifi_calloc_last={} free_count={}",
        label,
        primitive.malloc_internal_count,
        primitive.malloc_internal_last_size,
        primitive.calloc_internal_count,
        primitive.calloc_internal_last_size,
        primitive.wifi_malloc_count,
        primitive.wifi_malloc_last_size,
        primitive.wifi_calloc_count,
        primitive.wifi_calloc_last_size,
        primitive.free_count,
    );
    for idx in 0..primitive.task_get_current_task_recent_ordinals.len() {
        let ordinal = primitive.task_get_current_task_recent_ordinals[idx];
        let task_ptr = primitive.task_get_current_task_recent_ptrs[idx];
        if ordinal == 0 && task_ptr == 0 {
            continue;
        }
        println!(
            "nostd_wifi_control: wifi_adapter_primitive_diag_task_recent label={} idx={} ordinal={} task_ptr=0x{:08x} task_role={}",
            label,
            idx,
            ordinal,
            task_ptr,
            format_task_role(task_ptr),
        );
    }

    for idx in 0..diag.queue_send_sample_queues.len() {
        let queue = diag.queue_send_sample_queues[idx];
        let task = diag.queue_send_sample_tasks[idx];
        if queue == 0 && task == 0 {
            continue;
        }
        println!(
            "nostd_wifi_control: wifi_os_diag_send_sample label={} idx={} queue=0x{:08x} task=0x{:08x} task_role={} item_word0=0x{:08x} pointee_word0=0x{:08x} pointee_word1=0x{:08x}",
            label,
            idx,
            queue,
            task,
            format_task_role(task),
            diag.queue_send_sample_item_word0[idx],
            diag.queue_send_sample_item_pointee_word0[idx],
            diag.queue_send_sample_item_pointee_word1[idx],
        );
    }

    for idx in 0..diag.queue_send_recent_queues.len() {
        let ordinal = diag.queue_send_recent_ordinals[idx];
        let queue = diag.queue_send_recent_queues[idx];
        let task = diag.queue_send_recent_tasks[idx];
        if ordinal == 0 && queue == 0 && task == 0 {
            continue;
        }
        println!(
            "nostd_wifi_control: wifi_os_diag_send_recent label={} idx={} ordinal={} queue=0x{:08x} task=0x{:08x} task_role={} item_word0=0x{:08x} pointee_word0=0x{:08x} pointee_word1=0x{:08x}",
            label,
            idx,
            ordinal,
            queue,
            task,
            format_task_role(task),
            diag.queue_send_recent_item_word0[idx],
            diag.queue_send_recent_item_pointee_word0[idx],
            diag.queue_send_recent_item_pointee_word1[idx],
        );
    }

    for idx in 0..diag.queue_recv_sample_queues.len() {
        let queue = diag.queue_recv_sample_queues[idx];
        let task = diag.queue_recv_sample_tasks[idx];
        if queue == 0 && task == 0 {
            continue;
        }
        println!(
            "nostd_wifi_control: wifi_os_diag_recv_sample label={} idx={} queue=0x{:08x} task=0x{:08x} task_role={} item_word0=0x{:08x} pointee_word0=0x{:08x} pointee_word1=0x{:08x}",
            label,
            idx,
            queue,
            task,
            format_task_role(task),
            diag.queue_recv_sample_item_word0[idx],
            diag.queue_recv_sample_item_pointee_word0[idx],
            diag.queue_recv_sample_item_pointee_word1[idx],
        );
    }

    for idx in 0..diag.queue_recv_recent_queues.len() {
        let ordinal = diag.queue_recv_recent_ordinals[idx];
        let queue = diag.queue_recv_recent_queues[idx];
        let task = diag.queue_recv_recent_tasks[idx];
        if ordinal == 0 && queue == 0 && task == 0 {
            continue;
        }
        println!(
            "nostd_wifi_control: wifi_os_diag_recv_recent label={} idx={} ordinal={} queue=0x{:08x} task=0x{:08x} task_role={} item_word0=0x{:08x} pointee_word0=0x{:08x} pointee_word1=0x{:08x} caller_ptr=0x{:08x}",
            label,
            idx,
            ordinal,
            queue,
            task,
            format_task_role(task),
            diag.queue_recv_recent_item_word0[idx],
            diag.queue_recv_recent_item_pointee_word0[idx],
            diag.queue_recv_recent_item_pointee_word1[idx],
            diag.queue_recv_recent_caller_ptr[idx],
        );
    }
}

#[esp_hal::main]
fn main() -> ! {
    let minimal_log = minimal_log_requested();
    unsafe {
        __esp_rtos_diag_reset_legacy_task_model();
    }
    let peripherals = esp_hal::init(esp_hal::Config::default());
    esp_alloc::heap_allocator!(size: 64 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);
    diag_yield("after_rtos_start", 8);
    if !minimal_log {
        print_rtos_create_diag("after_rtos_start");
    }
    if !legacy_timer_compat_requested() {
        esp_rtos::precreate_esp_radio_timer_task();
        println!("nostd_wifi_control: precreate_timer_task=ok");
        diag_yield("after_precreate_timer_task", 8);
        if !minimal_log {
            print_rtos_create_diag("after_precreate_timer_task");
        }
    } else {
        println!("nostd_wifi_control: precreate_timer_task=skipped legacy_timer_compat=true");
    }

    println!("nostd_wifi_control: begin=true");

    let radio = match esp_radio::init() {
        Ok(ctrl) => ctrl,
        Err(err) => panic!("nostd_wifi_control: esp_radio::init err={:?}", err),
    };
    println!("nostd_wifi_control: esp_radio_init=ok");
    if legacy_timer_compat_requested() && legacy_timer_compat_init_tasks_requested() {
        esp_rtos::precreate_esp_radio_timer_task();
        println!("nostd_wifi_control: precreate_timer_task=after_esp_radio_init legacy_timer_compat=true");
        diag_yield("after_legacy_init_tasks_precreate_timer_task", 8);
        if !minimal_log {
            print_rtos_create_diag("after_legacy_init_tasks_precreate_timer_task");
        }
    }
    diag_yield("after_esp_radio_init", 8);
    if !minimal_log {
        print_rtos_create_diag("after_esp_radio_init");
    }
    esp_radio::diagnostic_wifi_os_diag_reset();
    esp_radio::diagnostic_reset_wifi_task_create_diag();
    esp_radio::diagnostic_reset_wifi_mac_isr_count();
    esp_radio::diagnostic_reset_wifi_promisc_rx_cb_count();
    esp_radio::wifi::diagnostic_reset_wifi_rx_cb_counts();

    let (mut controller, _ifaces) = match esp_radio::wifi::new(&radio, peripherals.WIFI, Default::default()) {
        Ok(parts) => parts,
        Err(err) => panic!("nostd_wifi_control: wifi_new err={:?}", err),
    };
    println!("nostd_wifi_control: wifi_new=ok");
    diag_yield("after_wifi_new", 8);
    if !minimal_log {
        print_rtos_create_diag("after_wifi_new");
        print_wifi_init_config_diag("after_wifi_new");
        print_wifi_task_create_diag("after_wifi_new");
        print_wifi_mac_isr_diag("after_wifi_new");
        print_wifi_rx_cb_diag("after_wifi_new");
        print_wifi_os_diag("after_wifi_new");
        print_timer_compat_diag("after_wifi_new");
        blob_state::print_blob_state("after_wifi_new");
        esp_radio::diagnostic_wifi_os_diag_reset();
    }

    if let Err(err) = controller.set_mode(WifiMode::Sta) {
        panic!("nostd_wifi_control: set_mode err={:?}", err);
    }
    println!("nostd_wifi_control: set_mode=sta");

    if let Err(err) = controller.start() {
        panic!("nostd_wifi_control: start err={:?}", err);
    }
    println!("nostd_wifi_control: start=ok");
    esp_radio::diagnostic_reset_wifi_mac_isr_count();
    println!(
        "nostd_wifi_control: wifi_mac_isr_diag label=before_promisc count={}",
        esp_radio::diagnostic_wifi_mac_isr_count()
    );
    print_wifi_promisc_cb_diag("before_promisc");
    diag_yield("after_wifi_start", 16);
    if !minimal_log {
        print_rtos_create_diag("after_wifi_start");
        print_wifi_task_create_diag("after_wifi_start");
        print_wifi_mac_isr_diag("after_wifi_start");
        print_wifi_rx_cb_diag("after_wifi_start");
        print_wifi_os_diag("after_wifi_start");
        print_timer_compat_diag("after_wifi_start");
        blob_state::print_blob_state("after_wifi_start");
        esp_radio::diagnostic_wifi_os_diag_reset();
    }
    if legacy_timer_compat_requested() && !legacy_timer_compat_init_tasks_requested() {
        esp_rtos::precreate_esp_radio_timer_task();
        println!("nostd_wifi_control: precreate_timer_task=late legacy_timer_compat=true");
        diag_yield("after_late_precreate_timer_task", 8);
        if !minimal_log {
            print_rtos_create_diag("after_late_precreate_timer_task");
        }
    }
    promisc_diag::run();
    println!(
        "nostd_wifi_control: wifi_mac_isr_diag label=after_promisc count={}",
        esp_radio::diagnostic_wifi_mac_isr_count()
    );
    print_wifi_promisc_cb_diag("after_promisc");
    print_wifi_os_diag("after_promisc");
    if !minimal_log {
        blob_state::print_blob_state("after_promisc");
    }

    let scan = controller.scan_with_config(ScanConfig::default().with_max(16));
    match scan {
        Ok(results) => {
            println!("nostd_wifi_control: scan=ok count={}", results.len());
            for (idx, ap) in results.iter().take(10).enumerate() {
                println!(
                    "nostd_wifi_control: ap idx={} ssid={} channel={} bssid={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} rssi={} auth={:?}",
                    idx,
                    ap.ssid,
                    ap.channel,
                    ap.bssid[0],
                    ap.bssid[1],
                    ap.bssid[2],
                    ap.bssid[3],
                    ap.bssid[4],
                    ap.bssid[5],
                    ap.signal_strength,
                    ap.auth_method,
                );
            }
        }
        Err(err) => {
            println!("nostd_wifi_control: scan=err err={:?}", err);
        }
    }
    println!(
        "nostd_wifi_control: wifi_mac_isr_diag label=after_scan count={}",
        esp_radio::diagnostic_wifi_mac_isr_count()
    );
    print_wifi_promisc_cb_diag("after_scan");
    if !minimal_log {
        print_wifi_mac_isr_diag("after_scan");
        print_wifi_rx_cb_diag("after_scan");
        print_wifi_os_diag("after_scan");
        print_timer_compat_diag("after_scan");
        blob_state::print_blob_state("after_scan");
    }

    match controller.stop() {
        Ok(()) => println!("nostd_wifi_control: stop=ok"),
        Err(err) => println!("nostd_wifi_control: stop=err err={:?}", err),
    }

    for idx in 0..64 {
        if idx % 8 == 0 {
            println!(
                "nostd_wifi_control: wifi_mac_isr_diag label=linger count={}",
                esp_radio::diagnostic_wifi_mac_isr_count()
            );
            print_wifi_promisc_cb_diag("linger");
        }
        diag_yield("linger", 64);
    }

    loop {
        core::hint::spin_loop();
    }
}
