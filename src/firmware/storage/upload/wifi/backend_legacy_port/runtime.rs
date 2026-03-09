use super::{
    legacy_runtime_config, runtime_bootstrap_status, RadioController, WifiController, WifiDevice,
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

    let radio_ctrl = match esp_radio::init() {
        Ok(ctrl) => ctrl,
        Err(err) => {
            println!("upload_http: legacy_port esp_radio::init err={:?}", err);
            return Err("asset-upload-http: legacy-port esp_radio::init failed");
        }
    };
    let radio_ctrl = RADIO_CTRL.init(radio_ctrl);
    let config = legacy_runtime_config(country_us_override);

    match esp_radio::wifi::new(radio_ctrl, wifi, config) {
        Ok((controller, ifaces)) => {
            println!("upload_http: legacy_port runtime_init result=ok");
            Ok((controller, ifaces.sta))
        }
        Err(err) => {
            println!("upload_http: legacy_port wifi init err={:?}", err);
            Err("asset-upload-http: legacy-port wifi init failed")
        }
    }
}
