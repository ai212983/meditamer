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
];

pub(crate) const LEGACY_MISSING_HOOKS: &[LegacyHookAvailability] = &[
    LegacyHookAvailability {
        name: "preempt::enable",
        available_from_current_stack: false,
        notes: "current esp_radio_rtos_driver only exposes initialized() and runtime primitives",
    },
    LegacyHookAvailability {
        name: "init_tasks",
        available_from_current_stack: false,
        notes: "legacy built-in scheduler task bootstrap has no current firmware-layer equivalent",
    },
    LegacyHookAvailability {
        name: "initial_yield",
        available_from_current_stack: false,
        notes: "current yield hooks exist, but not as the same explicit legacy bootstrap contract",
    },
];
