use std::{env, fs, path::PathBuf};

use event_config_compiler::generate_from_path;

fn env_flag_enabled(name: &str) -> bool {
    match env::var(name) {
        Ok(value) => value != "0" && !value.is_empty(),
        Err(_) => false,
    }
}

fn emit_legacy_wifi_link_wrappers() {
    println!("cargo:rustc-check-cfg=cfg(wifi_rx_recovery_minimal_diag)");
    println!("cargo:rustc-check-cfg=cfg(wifi_sniffer_passthrough_diag)");

    let minimal_rx_recovery = env_flag_enabled("MEDITAMER_WIFI_RX_RECOVERY_MINIMAL_DIAG")
        || env_flag_enabled("WIFI_RX_RECOVERY_MINIMAL_DIAG");

    if minimal_rx_recovery {
        println!("cargo:rustc-cfg=wifi_rx_recovery_minimal_diag");
    }
    if env_flag_enabled("MEDITAMER_WIFI_WDEV_SNIFFER_PASSTHROUGH_DIAG")
        || env_flag_enabled("WIFI_WDEV_SNIFFER_PASSTHROUGH_DIAG")
    {
        println!("cargo:rustc-cfg=wifi_sniffer_passthrough_diag");
    }

    let symbols: &[&str] = if minimal_rx_recovery {
        &[
            "wDev_ProcessFiq",
            "hal_mac_interrupt_get_event",
            "hal_mac_interrupt_clr_event",
            "wdevProcessRxSucDataAll",
        ]
    } else {
        &[
            "esp_rtos_timer_arm",
            "scan_pm_offchan",
            "scan_start",
            "scan_start_handler",
            "scan_set_scan_id",
            "scan_get_scan_id",
            "wifi_scan_start_process",
            "scan_enter_oper_channel_process",
            "scan_inter_channel_timeout_process",
            "clear_bss_queue",
            "ieee80211_sta_scan",
            "scan_hidden_ssid",
            "scan_profile_check",
            "scan_parse_beacon",
            "scan_set_current_scan_times",
            "scan_build_chan_list",
            "scan_set_desChan",
            "ieee80211_parse_wpa",
            "ieee80211_parse_rsn",
            "ieee80211_parse_wapi",
            "cnx_bss_alloc",
            "cnx_update_bss_more",
            "wdevProcessRxSucDataAll",
            "wDev_ProcessRxSucData",
            "ppProcessRxPktHdr",
            "ppRxPkt",
            "ppRxProtoProc",
            "ppRxFragmentProc",
            "ppEnqueueRxq",
            "ppDequeueRxq_Locked",
            "lmacRxDone",
            "wDev_ProcessFiq",
            "wdev_process_panic_watchdog",
            "hal_mac_rx_get_end_info",
            "hal_mac_interrupt_get_event",
            "hal_mac_interrupt_clr_event",
            "pp_post",
            "lmacProcessRxSucData",
            "sta_input",
            "sta_rx_cb",
            "sta_recv_mgmt",
            "ieee80211_regdomain_chan_in_range",
            "ieee80211_regdomain_min_chan",
            "ieee80211_regdomain_max_chan",
            "nan_dp_schedule_ndc_start",
            "_do_wifi_start",
            "wifi_hw_start",
            "chm_init",
        ]
    };

    for symbol in symbols {
        println!("cargo:rustc-link-arg=-Wl,--wrap={symbol}");
    }

    for symbol in [
        "keep_wdev_branch_trampolines",
        "wdev_process_panic_watchdog_trampoline",
        "lmac_process_rx_suc_data_trampoline",
        "pp_post_trampoline",
        "wdev_process_rx_suc_data_trampoline",
    ] {
        println!("cargo:rustc-link-arg=-Wl,--undefined={symbol}");
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=MEDITAMER_WIFI_LEGACY_LINK_WRAPS");
    if env_flag_enabled("MEDITAMER_WIFI_LEGACY_LINK_WRAPS") {
        emit_legacy_wifi_link_wrappers();
    }

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let config_path = manifest_dir.join("config/events.toml");

    println!("cargo:rerun-if-changed={}", config_path.display());

    let generated = generate_from_path(&config_path).unwrap_or_else(|e| {
        panic!(
            "event config compile failed for {}: {e}",
            config_path.display()
        )
    });

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR"));
    let out_file = out_dir.join("event_config.rs");
    fs::write(&out_file, generated)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", out_file.display()));
}
