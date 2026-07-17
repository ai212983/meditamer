use core::cell::RefCell;

use embassy_sync::blocking_mutex::{raw::CriticalSectionRawMutex, Mutex};
use esp_hal::timer::timg::Timer;

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
    pub(crate) current_core_initialized: bool,
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
    let status = esp_rtos::legacy_preempt_bootstrap_compat();
    LegacyBootstrapRuntimeStatus {
        scheduler_initialized: status.scheduler_initialized,
        current_core_initialized: status.current_core_initialized,
        timer_task_precreated: status.timer_task_precreated,
        timer_task_started: status.timer_task_started,
        yielded_once: status.yielded_once,
    }
}

pub(crate) fn legacy_timer_compat_init_tasks_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_TIMER_COMPAT_INIT_TASKS_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RADIO_USE_LEGACY_TIMER_COMPAT_INIT_TASKS_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

static LEGACY_PREEMPT_TIMER: Mutex<CriticalSectionRawMutex, RefCell<Option<Timer<'static>>>> =
    Mutex::new(RefCell::new(None));

pub(crate) fn install_legacy_preempt_timer(timer: Timer<'static>) {
    LEGACY_PREEMPT_TIMER.lock(|shared| {
        let slot = &mut *shared.borrow_mut();
        if slot.is_none() {
            *slot = Some(timer);
        }
    });
}

pub(crate) fn setup_legacy_preempt_timer() -> bool {
    let timer = LEGACY_PREEMPT_TIMER.lock(|shared| shared.borrow_mut().take());
    let Some(timer) = timer else {
        return false;
    };
    esp_rtos::legacy_preempt_builtin_setup_timer(timer);
    true
}
