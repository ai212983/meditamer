#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LegacyBootstrapStep {
    EnableWifiPowerDomain,
    PhyMemInit,
    SetupRadioIsr,
    SchedulerEnable,
    InitTasks,
    InitialYield,
    WifiSetLogVerbose,
    InitRadioClocks,
    CoexInitialize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LegacySchedulerContract {
    pub(crate) requires_explicit_enable: bool,
    pub(crate) requires_task_bootstrap: bool,
    pub(crate) requires_initial_yield: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LegacyBootstrapRuntimeStatus {
    pub(crate) scheduler_initialized: bool,
    pub(crate) timer_task_precreated: bool,
    pub(crate) timer_task_started: bool,
    pub(crate) yielded_once: bool,
}

pub(crate) const LEGACY_SCHEDULER_CONTRACT: LegacySchedulerContract = LegacySchedulerContract {
    requires_explicit_enable: true,
    requires_task_bootstrap: true,
    requires_initial_yield: true,
};

pub(crate) const LEGACY_BOOTSTRAP_SEQUENCE: &[LegacyBootstrapStep] = &[
    LegacyBootstrapStep::EnableWifiPowerDomain,
    LegacyBootstrapStep::PhyMemInit,
    LegacyBootstrapStep::SetupRadioIsr,
    LegacyBootstrapStep::SchedulerEnable,
    LegacyBootstrapStep::InitTasks,
    LegacyBootstrapStep::InitialYield,
    LegacyBootstrapStep::WifiSetLogVerbose,
    LegacyBootstrapStep::InitRadioClocks,
    LegacyBootstrapStep::CoexInitialize,
];

pub(crate) fn runtime_bootstrap_status() -> LegacyBootstrapRuntimeStatus {
    let status = esp_rtos::bootstrap_legacy_wifi_contract_shim();
    LegacyBootstrapRuntimeStatus {
        scheduler_initialized: status.scheduler_initialized,
        timer_task_precreated: status.timer_task_precreated,
        timer_task_started: status.timer_task_started,
        yielded_once: status.yielded_once,
    }
}
