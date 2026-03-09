#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LegacyHookAvailability {
    pub(crate) name: &'static str,
    pub(crate) available_from_current_stack: bool,
    pub(crate) notes: &'static str,
}

pub(crate) const LEGACY_AVAILABLE_HOOKS: &[LegacyHookAvailability] = &[
    LegacyHookAvailability {
        name: "enable_wifi_power_domain",
        available_from_current_stack: true,
        notes: "current esp-radio exposes the equivalent internal step during init",
    },
    LegacyHookAvailability {
        name: "phy_mem_init",
        available_from_current_stack: true,
        notes: "current vendored stack already contains the legacy ESP32 PHY init helper",
    },
    LegacyHookAvailability {
        name: "setup_radio_isr",
        available_from_current_stack: true,
        notes: "current ESP32 ISR setup is effectively identical to legacy",
    },
    LegacyHookAvailability {
        name: "wifi_set_log_verbose",
        available_from_current_stack: true,
        notes: "current stack performs this during esp_radio::init",
    },
    LegacyHookAvailability {
        name: "init_radio_clocks",
        available_from_current_stack: true,
        notes: "current stack performs this during esp_radio::init",
    },
    LegacyHookAvailability {
        name: "coex_initialize",
        available_from_current_stack: true,
        notes: "current stack performs this during esp_radio::init when coex is enabled",
    },
    LegacyHookAvailability {
        name: "bootstrap_legacy_wifi_contract_shim",
        available_from_current_stack: true,
        notes: "new esp-rtos shim can precreate timer task and yield once after scheduler init",
    },
    LegacyHookAvailability {
        name: "preempt::enable",
        available_from_current_stack: true,
        notes: "legacy-preempt compat now validates scheduler/current-core readiness in esp-rtos",
    },
    LegacyHookAvailability {
        name: "init_tasks",
        available_from_current_stack: true,
        notes: "legacy-preempt compat now precreates the timer task and waits for first entry",
    },
    LegacyHookAvailability {
        name: "initial_yield",
        available_from_current_stack: true,
        notes: "legacy-preempt compat now exposes an explicit initial yield step",
    },
];

pub(crate) const LEGACY_MISSING_HOOKS: &[LegacyHookAvailability] = &[];
