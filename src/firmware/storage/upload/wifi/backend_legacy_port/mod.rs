#![allow(dead_code, unused_imports)]

mod availability;
mod bootstrap;
mod config;
mod contracts;
mod controller;
mod runtime;
mod types;

pub(crate) use availability::{
    LegacyHookAvailability, LEGACY_AVAILABLE_HOOKS, LEGACY_MISSING_HOOKS,
};
pub(crate) use bootstrap::{
    runtime_bootstrap_status, LegacyBootstrapRuntimeStatus, LegacyBootstrapStep,
    LegacySchedulerContract, LEGACY_BOOTSTRAP_SEQUENCE, LEGACY_SCHEDULER_CONTRACT,
};
pub(crate) use config::runtime_config as legacy_runtime_config;
pub(crate) use contracts::{
    LegacyInitConfigContract, LegacyPortScope, LegacyWifiTaskContract, LEGACY_INIT_CONFIG_CONTRACT,
    LEGACY_PORT_SCOPE, LEGACY_WIFI_TASK_CONTRACT,
};
pub(crate) use controller::{
    scan_with_config as controller_scan_with_config, start as controller_start,
    stop as controller_stop,
};
pub(crate) use runtime::{
    initialize_runtime_sta_legacy_port, legacy_port_runtime_enabled, LEGACY_RUNTIME_NAME,
};
pub(crate) use types::{
    AccessPointInfo, RadioController, ScanConfig, WifiController, WifiDevice, WifiError,
};

pub(crate) const STAGE_NAME: &str = "bootstrap-contract-staging";
pub(crate) const BLOCKER: &str =
    "legacy runtime behavior is proven in standalone control, but not yet ported into firmware";
