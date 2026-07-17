use super::{
    legacy_runtime_config, legacy_timer_compat_init_tasks_enabled, runtime_bootstrap_status,
    setup_legacy_preempt_timer, RadioController, WifiController, WifiDevice,
    LEGACY_BOOTSTRAP_SEQUENCE, LEGACY_INIT_CONFIG_CONTRACT, LEGACY_SCHEDULER_CONTRACT,
    LEGACY_WIFI_TASK_CONTRACT,
};
use esp_hal::peripherals::WIFI;
use esp_println::println;
use static_cell::StaticCell;

mod state;

pub(crate) use state::log_runtime_state;

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
