use esp_radio_rtos_driver::Scheduler as _;

use crate::{SCHEDULER, task};

use super::{ensure_timer_task, timer_task_entry_count};

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LegacyPreemptCompatStatus {
    pub scheduler_initialized: bool,
    pub current_core_initialized: bool,
    pub timer_task_precreated: bool,
    pub timer_task_started: bool,
    pub yielded_once: bool,
}

fn scheduler_and_core_initialized() -> (bool, bool) {
    let scheduler_initialized = SCHEDULER.initialized();
    let current_core_initialized = if scheduler_initialized {
        SCHEDULER.with(|scheduler| {
            scheduler.per_cpu[esp_hal::system::Cpu::current() as usize].initialized
        })
    } else {
        false
    };
    (scheduler_initialized, current_core_initialized)
}

pub fn legacy_preempt_bootstrap_compat() -> LegacyPreemptCompatStatus {
    let (scheduler_initialized, current_core_initialized) = scheduler_and_core_initialized();
    if !scheduler_initialized || !current_core_initialized {
        return LegacyPreemptCompatStatus {
            scheduler_initialized,
            current_core_initialized,
            timer_task_precreated: false,
            timer_task_started: false,
            yielded_once: false,
        };
    }

    let entries_before = timer_task_entry_count();
    ensure_timer_task();

    let mut timer_task_started = false;
    let mut yielded_once = false;
    for _ in 0..8 {
        task::yield_task();
        yielded_once = true;
        if timer_task_entry_count() > entries_before {
            timer_task_started = true;
            break;
        }
    }

    LegacyPreemptCompatStatus {
        scheduler_initialized: true,
        current_core_initialized: true,
        timer_task_precreated: true,
        timer_task_started,
        yielded_once,
    }
}
