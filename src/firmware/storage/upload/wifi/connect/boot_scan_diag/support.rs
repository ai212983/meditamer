use super::{log_boot_scan_only_core_counters, log_boot_scan_only_runtime_counters};
use crate::firmware::storage::upload::wifi::{
    backend_legacy_port,
    connect::{
        blob_state_diag::log_blob_state_diag,
        bss_wrap_diag::{log_bss_wrap_diag, reset_bss_wrap_diag},
        lmac_wrap_diag::{log_lmac_wrap_diag, reset_lmac_wrap_diag},
        nan_timer_redirect_diag::{log_nan_timer_redirect_diag, reset_nan_timer_redirect_diag},
        nan_timer_slot_retarget_diag::{
            log_nan_timer_slot_retarget_diag, reset_nan_timer_slot_retarget_diag,
        },
        parse_wrap_diag::{log_parse_wrap_diag, reset_parse_wrap_diag},
        profile_wrap_diag::{log_profile_wrap_diag, reset_profile_wrap_diag},
        rx_dispatch_wrap_diag::{log_rx_dispatch_wrap_diag, reset_rx_dispatch_wrap_diag},
        scan_cmd_helper_wrap_diag::{
            log_scan_cmd_helper_wrap_diag, reset_scan_cmd_helper_wrap_diag,
        },
        scan_process_wrap_diag::{log_scan_process_wrap_diag, reset_scan_process_wrap_diag},
        sta_recv_wrap_diag::{log_sta_recv_wrap_diag, reset_sta_recv_wrap_diag},
        sta_rx_cb_wrap_diag::{log_sta_rx_cb_wrap_diag, reset_sta_rx_cb_wrap_diag},
        start_path_wrap_diag::{log_start_path_wrap_diag, reset_start_path_wrap_diag},
        timer_arm_wrap_diag::{log_timer_arm_wrap_diag, reset_timer_arm_wrap_diag},
        wdev_branch_wrap_diag::{log_wdev_branch_wrap_diag, reset_wdev_branch_wrap_diag},
        wdev_fiq_wrap_diag::{log_wdev_fiq_wrap_diag, reset_wdev_fiq_wrap_diag},
        wdev_sniffer_probe_trampoline::{
            log_wdev_sniffer_probe_trampoline, reset_wdev_sniffer_probe_trampoline,
        },
        wdev_sniffer_wrap_diag::{log_wdev_sniffer_wrap_diag, reset_wdev_sniffer_wrap_diag},
    },
};

#[cfg(not(wifi_rx_recovery_minimal_diag))]
use crate::firmware::storage::upload::wifi::connect::wdev_process_rx_wrap_diag::{
    log_wdev_process_rx_wrap_diag, reset_wdev_process_rx_wrap_diag,
};

pub(super) fn boot_scan_only_diag_idf_explicit_first_delay_ms() -> u64 {
    match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_FIRST_DELAY_MS") {
        Some(value) => value.parse::<u64>().unwrap_or(0),
        None => match option_env!("WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_FIRST_DELAY_MS") {
            Some(value) => value.parse::<u64>().unwrap_or(0),
            None => 0,
        },
    }
}

pub(super) fn log_boot_scan_only_diag_counters(stage: &str) {
    log_blob_state_diag(stage);
    log_parse_wrap_diag(stage);
    log_bss_wrap_diag(stage);
    log_profile_wrap_diag(stage);
    log_lmac_wrap_diag(stage);
    log_wdev_fiq_wrap_diag(stage);
    log_wdev_branch_wrap_diag(stage);
    log_wdev_sniffer_wrap_diag(stage);
    log_wdev_sniffer_probe_trampoline(stage);
    #[cfg(not(wifi_rx_recovery_minimal_diag))]
    log_wdev_process_rx_wrap_diag(stage);
    log_rx_dispatch_wrap_diag(stage);
    log_scan_cmd_helper_wrap_diag(stage);
    log_scan_process_wrap_diag(stage);
    log_nan_timer_redirect_diag(stage);
    log_nan_timer_slot_retarget_diag(stage);
    log_start_path_wrap_diag(stage);
    log_sta_rx_cb_wrap_diag(stage);
    log_sta_recv_wrap_diag(stage);
    log_timer_arm_wrap_diag(stage);
    log_boot_scan_only_core_counters(stage);
    log_boot_scan_only_runtime_counters(stage);
    backend_legacy_port::log_runtime_state(stage);
}

pub(super) fn reset_boot_scan_only_diag_counters() {
    esp_radio::diagnostic_reset_wifi_mac_isr_count();
    esp_radio::diagnostic_wifi_os_diag_reset();
    esp_radio::diagnostic_reset_phy_common_clock_diag();
    esp_radio::wifi::diagnostic_reset_wifi_rx_cb_counts();
    esp_radio::diagnostic_reset_wifi_scan_done_eventpost_diag();
    esp_radio::diagnostic_reset_timer_compat_diag();
    esp_radio::diagnostic_reset_timer_callback_exec_diag();
    esp_radio::diagnostic_reset_scheduler_timer_wake_diag();
    esp_radio::diagnostic_reset_legacy_builtin_scheduler_diag();
    esp_rtos::diagnostic_esp_radio_sem_trace_reset();
    reset_parse_wrap_diag();
    reset_bss_wrap_diag();
    reset_profile_wrap_diag();
    reset_lmac_wrap_diag();
    reset_wdev_fiq_wrap_diag();
    reset_wdev_branch_wrap_diag();
    reset_wdev_sniffer_wrap_diag();
    reset_wdev_sniffer_probe_trampoline();
    #[cfg(not(wifi_rx_recovery_minimal_diag))]
    reset_wdev_process_rx_wrap_diag();
    reset_rx_dispatch_wrap_diag();
    reset_scan_cmd_helper_wrap_diag();
    reset_scan_process_wrap_diag();
    reset_nan_timer_redirect_diag();
    reset_nan_timer_slot_retarget_diag();
    reset_start_path_wrap_diag();
    reset_sta_rx_cb_wrap_diag();
    reset_sta_recv_wrap_diag();
    reset_timer_arm_wrap_diag();
}
