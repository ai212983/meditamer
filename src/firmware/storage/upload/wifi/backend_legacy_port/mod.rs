#![allow(dead_code, unused_imports)]

mod availability;
mod bootstrap;
mod config;
mod contracts;
mod controller;
mod runtime;
mod types;
mod wifi_new;

pub(crate) use availability::{
    LegacyHookAvailability, LEGACY_AVAILABLE_HOOKS, LEGACY_MISSING_HOOKS,
};
pub(crate) use bootstrap::{
    install_legacy_preempt_timer, legacy_timer_compat_init_tasks_enabled, runtime_bootstrap_status,
    setup_legacy_preempt_timer, LegacyBootstrapRuntimeStatus, LegacyBootstrapStep,
    LegacySchedulerContract, LEGACY_BOOTSTRAP_SEQUENCE, LEGACY_SCHEDULER_CONTRACT,
};
pub(crate) use config::{
    active_scan_config as legacy_active_scan_config,
    channel_active_scan_config as legacy_channel_active_scan_config,
    client_mode_config as legacy_client_mode_config,
    directed_active_scan_config as legacy_directed_active_scan_config,
    error_is_no_mem as legacy_error_is_no_mem, passive_scan_config as legacy_passive_scan_config,
    power_save_none as legacy_power_save_none,
    raw_broad_scan_config as legacy_raw_broad_scan_config, runtime_config as legacy_runtime_config,
    sta_mode as legacy_sta_mode, standard_bgn_protocols as legacy_standard_bgn_protocols,
};
pub(crate) use contracts::{
    LegacyInitConfigContract, LegacyPortScope, LegacyWifiTaskContract, LEGACY_INIT_CONFIG_CONTRACT,
    LEGACY_PORT_SCOPE, LEGACY_WIFI_TASK_CONTRACT,
};
pub(crate) use controller::{
    connect as controller_connect, disconnect as controller_disconnect,
    is_started as controller_is_started, rssi as controller_rssi,
    scan_with_config as controller_scan_with_config, set_config as controller_set_config,
    set_mode as controller_set_mode, set_power_saving as controller_set_power_saving,
    set_protocol as controller_set_protocol, start as controller_start, stop as controller_stop,
};
pub(crate) use runtime::{
    initialize_runtime_sta_legacy_port, legacy_port_runtime_enabled, log_runtime_state,
    LEGACY_RUNTIME_NAME,
};
pub(crate) use types::{
    AccessPointInfo, ModeConfig, PowerSaveMode, Protocol, RadioController, ScanConfig,
    WifiController, WifiDevice, WifiError, WifiMode,
};

pub(crate) const STAGE_NAME: &str = "bootstrap-contract-staging";
pub(crate) const BLOCKER: &str =
    "legacy runtime behavior is proven in standalone control, but not yet ported into firmware";
