use esp_radio_rtos_driver::Scheduler as _;

use crate::{SCHEDULER, task};

use super::{ensure_timer_task, timer_task_entry_count};

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LegacyWifiBootstrapShimStatus {
    pub scheduler_initialized: bool,
    pub timer_task_precreated: bool,
    pub timer_task_started: bool,
    pub yielded_once: bool,
}

pub fn bootstrap_legacy_wifi_contract_shim() -> LegacyWifiBootstrapShimStatus {
    let scheduler_initialized = SCHEDULER.initialized();
    if !scheduler_initialized {
        return LegacyWifiBootstrapShimStatus {
            scheduler_initialized: false,
            timer_task_precreated: false,
            timer_task_started: false,
            yielded_once: false,
        };
    }

    let entries_before = timer_task_entry_count();
    ensure_timer_task();
    let mut yielded_once = false;
    let mut timer_task_started = false;
    for _ in 0..8 {
        task::yield_task();
        yielded_once = true;
        if timer_task_entry_count() > entries_before {
            timer_task_started = true;
            break;
        }
    }

    LegacyWifiBootstrapShimStatus {
        scheduler_initialized: true,
        timer_task_precreated: true,
        timer_task_started,
        yielded_once,
    }
}
