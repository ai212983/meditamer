pub(crate) fn backend_legacy_port_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn env_enabled(name: &str) -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_QUEUE_SEND_FROM_ISR_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) && name == "queue_send_from_isr"
        || matches!(
            option_env!("ESP_RADIO_USE_LEGACY_QUEUE_SEND_FROM_ISR_DIAG"),
            Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
        ) && name == "queue_send_from_isr"
        || matches!(
            option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_SEMAPHORE_FROM_ISR_DIAG"),
            Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
        ) && name == "semaphore_from_isr"
        || matches!(
            option_env!("ESP_RADIO_USE_LEGACY_SEMAPHORE_FROM_ISR_DIAG"),
            Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
        ) && name == "semaphore_from_isr"
        || matches!(
            option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_TASK_YIELD_FROM_ISR_DIAG"),
            Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
        ) && name == "task_yield_from_isr"
        || matches!(
            option_env!("ESP_RADIO_USE_LEGACY_TASK_YIELD_FROM_ISR_DIAG"),
            Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
        ) && name == "task_yield_from_isr"
        || matches!(
            option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_COEX_STATUS_GET_DIAG"),
            Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
        ) && name == "coex_status_get"
        || matches!(
            option_env!("ESP_RADIO_USE_LEGACY_COEX_STATUS_GET_DIAG"),
            Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
        ) && name == "coex_status_get"
}

pub(crate) fn legacy_queue_send_from_isr_enabled() -> bool {
    backend_legacy_port_enabled() || env_enabled("queue_send_from_isr")
}

pub(crate) fn legacy_semaphore_from_isr_enabled() -> bool {
    backend_legacy_port_enabled() || env_enabled("semaphore_from_isr")
}

pub(crate) fn legacy_task_yield_from_isr_enabled() -> bool {
    backend_legacy_port_enabled() || env_enabled("task_yield_from_isr")
}

pub(crate) fn legacy_coex_status_get_enabled() -> bool {
    backend_legacy_port_enabled() || env_enabled("coex_status_get")
}

pub(crate) fn legacy_blocking_semaphore_ticks_enabled() -> bool {
    backend_legacy_port_enabled()
}

pub(crate) fn legacy_blocking_queue_ticks_enabled() -> bool {
    backend_legacy_port_enabled()
}

pub(crate) fn legacy_vtaskdelay_ticks_enabled() -> bool {
    backend_legacy_port_enabled()
}
