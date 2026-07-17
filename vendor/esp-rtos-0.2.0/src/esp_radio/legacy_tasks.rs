use crate::task::yield_task;

use super::timer_queue::ensure_legacy_timer_task;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LegacyWifiTasksInitStatus {
    pub timer_task_precreated: bool,
    pub yielded_once: bool,
}

pub fn init_legacy_wifi_tasks() -> LegacyWifiTasksInitStatus {
    // Legacy esp-wifi explicitly creates the timer task during init_tasks()
    // and yields once before continuing Wi-Fi bring-up.
    ensure_legacy_timer_task();
    yield_task();
    LegacyWifiTasksInitStatus {
        timer_task_precreated: true,
        yielded_once: true,
    }
}
