#![allow(dead_code, unused_imports)]

mod availability;
mod bootstrap;
mod contracts;
mod runtime;

pub(crate) use availability::{
    LegacyHookAvailability, LEGACY_AVAILABLE_HOOKS, LEGACY_MISSING_HOOKS,
};
pub(crate) use bootstrap::{
    runtime_bootstrap_status, LegacyBootstrapRuntimeStatus, LegacyBootstrapStep,
    LegacySchedulerContract, LEGACY_BOOTSTRAP_SEQUENCE, LEGACY_SCHEDULER_CONTRACT,
};
pub(crate) use contracts::{
    LegacyInitConfigContract, LegacyPortScope, LegacyWifiTaskContract, LEGACY_INIT_CONFIG_CONTRACT,
    LEGACY_PORT_SCOPE, LEGACY_WIFI_TASK_CONTRACT,
};
pub(crate) use runtime::{
    initialize_runtime_sta_legacy_port, legacy_port_runtime_enabled, LEGACY_RUNTIME_NAME,
};

pub(crate) const STAGE_NAME: &str = "bootstrap-contract-staging";
pub(crate) const BLOCKER: &str =
    "legacy runtime behavior is proven in standalone control, but not yet ported into firmware";
