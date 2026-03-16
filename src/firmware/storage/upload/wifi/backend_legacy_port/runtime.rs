use super::{
    legacy_runtime_config, legacy_timer_compat_init_tasks_enabled, runtime_bootstrap_status,
    setup_legacy_preempt_timer, RadioController, WifiController, WifiDevice,
    LEGACY_BOOTSTRAP_SEQUENCE, LEGACY_INIT_CONFIG_CONTRACT, LEGACY_SCHEDULER_CONTRACT,
    LEGACY_WIFI_TASK_CONTRACT,
};
use esp_hal::peripherals::WIFI;
use esp_println::println;
use static_cell::StaticCell;

pub(crate) const LEGACY_RUNTIME_NAME: &str = "backend-legacy-port";

pub(crate) fn legacy_port_runtime_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn log_runtime_state(stage: &str) {
    if !legacy_port_runtime_enabled() {
        return;
    }

    let os_diag = esp_radio::diagnostic_wifi_os_diag_snapshot();
    let adapter_diag = esp_radio::diagnostic_wifi_adapter_primitive_diag();
    let scan_done = esp_radio::diagnostic_wifi_scan_done_eventpost_diag();
    let task_create = esp_radio::diagnostic_wifi_task_create_diag();
    let init_config = esp_radio::diagnostic_wifi_init_config_diag();
    let legacy_builtin = esp_radio::diagnostic_legacy_builtin_scheduler_diag();
    let legacy_preempt = esp_radio::diagnostic_legacy_preempt_builtin_diag();

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
    println!(
        "upload_http: legacy_port runtime_state_task after={} task_create_count={} recent_name_tag0=0x{:08x} recent_stack0={} recent_prio0={} recent_core0={} recent_task_ptr0=0x{:x} init_config_ptr=0x{:x} osi_funcs_ptr=0x{:x} wifi_task_core_id={} init_magic=0x{:08x} osi_queue_create_ptr=0x{:x} osi_queue_recv_ptr=0x{:x} osi_task_create_ptr=0x{:x} osi_task_create_pinned_ptr=0x{:x} osi_task_get_current_ptr=0x{:x} osi_thread_sem_get_ptr=0x{:x} osi_event_post_ptr=0x{:x} osi_malloc_internal_ptr=0x{:x}",
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
    );
}

fn log_legacy_runtime_contract() {
    println!(
        "upload_http: legacy_port_runtime name={} bootstrap_steps={} requires_enable={} requires_task_bootstrap={} requires_initial_yield={} init_static_rx={} init_dynamic_rx={} init_rx_mgmt={} init_rx_ba_win={} task_name={} task_stack={} task_prio={} task_core={}",
        LEGACY_RUNTIME_NAME,
        LEGACY_BOOTSTRAP_SEQUENCE.len(),
        LEGACY_SCHEDULER_CONTRACT.requires_explicit_enable,
        LEGACY_SCHEDULER_CONTRACT.requires_task_bootstrap,
        LEGACY_SCHEDULER_CONTRACT.requires_initial_yield,
        LEGACY_INIT_CONFIG_CONTRACT.static_rx_buf_num,
        LEGACY_INIT_CONFIG_CONTRACT.dynamic_rx_buf_num,
        LEGACY_INIT_CONFIG_CONTRACT.rx_mgmt_buf_num,
        LEGACY_INIT_CONFIG_CONTRACT.rx_ba_win,
        LEGACY_WIFI_TASK_CONTRACT.name,
        LEGACY_WIFI_TASK_CONTRACT.stack_size,
        LEGACY_WIFI_TASK_CONTRACT.requested_priority,
        LEGACY_WIFI_TASK_CONTRACT.core,
    );
}

fn log_bootstrap_status() {
    let status = runtime_bootstrap_status();
    println!(
        "upload_http: legacy_port_bootstrap scheduler_initialized={} current_core_initialized={} timer_task_precreated={} timer_task_started={} yielded_once={}",
        status.scheduler_initialized,
        status.current_core_initialized,
        status.timer_task_precreated,
        status.timer_task_started,
        status.yielded_once,
    );
}

pub(crate) fn initialize_runtime_sta_legacy_port(
    wifi: WIFI<'static>,
    country_us_override: bool,
) -> Result<(WifiController<'static>, WifiDevice<'static>), &'static str> {
    static RADIO_CTRL: StaticCell<RadioController> = StaticCell::new();

    log_legacy_runtime_contract();
    log_bootstrap_status();
    println!("upload_http: legacy_port runtime_init stage=before_esp_radio_init");
    esp_radio::backend_legacy_port_enable_scheduler();

    match esp_radio::backend_legacy_port_init_pre_tasks() {
        Ok(()) => {}
        Err(err) => {
            println!(
                "upload_http: legacy_port esp_radio::backend_legacy_port_init_pre_tasks err={:?}",
                err
            );
            return Err("asset-upload-http: legacy-port pre-tasks init failed");
        }
    };
    println!("upload_http: legacy_port runtime_init stage=after_esp_radio_init");
    log_runtime_state("after_init_pre_tasks");
    println!(
        "upload_http: legacy_port setup_timer result={}",
        if setup_legacy_preempt_timer() {
            "ok"
        } else {
            "missing"
        }
    );
    if legacy_timer_compat_init_tasks_enabled() {
        let status = esp_radio::backend_legacy_port_init_tasks();
        println!(
            "upload_http: legacy_port init_tasks result=ok timer_task_precreated={} yielded_once={}",
            status.timer_task_precreated,
            status.yielded_once,
        );
        log_runtime_state("after_init_tasks");
    }
    let radio_ctrl = match esp_radio::backend_legacy_port_init_post_tasks() {
        Ok(ctrl) => ctrl,
        Err(err) => {
            println!(
                "upload_http: legacy_port esp_radio::backend_legacy_port_init_post_tasks err={:?}",
                err
            );
            return Err("asset-upload-http: legacy-port post-tasks init failed");
        }
    };
    log_runtime_state("after_init_post_tasks");
    let radio_ctrl = RADIO_CTRL.init(radio_ctrl);
    let config = legacy_runtime_config(country_us_override);
    println!("upload_http: legacy_port runtime_init stage=before_wifi_new");

    match super::wifi_new::wifi_new_legacy(radio_ctrl, wifi, config) {
        Ok((controller, ifaces)) => {
            log_runtime_state("after_wifi_new");
            println!("upload_http: legacy_port runtime_init result=ok");
            Ok((controller, ifaces.sta))
        }
        Err(err) => {
            println!("upload_http: legacy_port wifi init err={:?}", err);
            Err("asset-upload-http: legacy-port wifi init failed")
        }
    }
}
