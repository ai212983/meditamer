use portable_atomic::{AtomicU32, Ordering};

use crate::{task, task::TaskPtr};

static WIFI_TASK_SELECTED_COUNT: AtomicU32 = AtomicU32::new(0);

fn legacy_wifi_task_bootstrap_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_WIFI_TASK_BOOTSTRAP_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RTOS_USE_LEGACY_WIFI_TASK_BOOTSTRAP_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn is_wifi_task(task: TaskPtr) -> bool {
    let role = unsafe { task.as_ref().task_role };
    role[..4] == *b"wifi"
}

pub(crate) fn note_task_selected(task: TaskPtr) {
    if is_wifi_task(task) {
        WIFI_TASK_SELECTED_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn wifi_task_selected_count() -> u32 {
    WIFI_TASK_SELECTED_COUNT.load(Ordering::Relaxed)
}

pub(crate) fn maybe_handoff_to_wifi_task(name: &str) {
    if !legacy_wifi_task_bootstrap_diag_enabled() || name != "wifi" {
        return;
    }

    let before = wifi_task_selected_count();
    for _ in 0..16 {
        task::yield_task();
        if wifi_task_selected_count() > before {
            break;
        }
    }
}
