//! Emits legacy Wi-Fi link-wrapper flags for the `meditamer` binary, and
//! regenerates `ota_build_config.rs` for `src/updater/mod.rs`'s own
//! `include!`.
//!
//! Split out of the root package's `build.rs` in the product and target axis
//! completion plan's Phase 3. The link-wrapper flags stayed a target concern
//! rather than moving to `products/meditamer`: `cargo:rustc-link-arg` only
//! has an effect for the crate producing the final linked binary, never for
//! a library, so it could not have moved to the product crate even though
//! the env vars it reads are otherwise unrelated to chip startup. See
//! `products/meditamer/build.rs` for the event-config/ambient-font
//! generation that did move there outright, and `write_ota_build_config`'s
//! own doc comment below for why *that* piece is generated in both places.

use std::{env, fs, path::PathBuf};

const OTA_PUBLIC_KEY_ENV: &str = "MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX";
const OTA_BUILD_ID_ENV: &str = "MEDITAMER_FIRMWARE_BUILD_ID";

fn decode_hex_byte(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

/// `targets/meditamer-inkplate/src/updater/mod.rs`'s own copy of the same
/// `include!(concat!(env!("OUT_DIR"), "/ota_build_config.rs"))` that
/// `products/meditamer/src/firmware/update.rs` uses (see that crate's
/// `build.rs`) -- each `include!` resolves against the `OUT_DIR` of the
/// crate the including file is compiled as part of, and the updater moved
/// to this crate in Phase 3 while `update.rs` stayed in the product, so the
/// generated file has to exist in both `OUT_DIR`s now. Keeping this
/// function's body identical between the two `build.rs` files is what keeps
/// the two copies consistent for a given build invocation, since both read
/// the same two env vars.
fn write_ota_build_config(out_dir: &std::path::Path) {
    println!("cargo:rerun-if-env-changed={OTA_PUBLIC_KEY_ENV}");
    println!("cargo:rerun-if-env-changed={OTA_BUILD_ID_ENV}");
    let configured = env::var(OTA_PUBLIC_KEY_ENV).ok();
    let build_id = env::var(OTA_BUILD_ID_ENV).unwrap_or_else(|_| "unlabeled".to_owned());
    assert!(
        !build_id.is_empty()
            && build_id.len() <= 31
            && build_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')),
        "{OTA_BUILD_ID_ENV} must be 1-31 ASCII letters, digits, '.', '_' or '-'"
    );
    let mut key = [0u8; 32];
    if let Some(raw) = configured.as_deref() {
        let bytes = raw.as_bytes();
        assert_eq!(
            bytes.len(),
            64,
            "{OTA_PUBLIC_KEY_ENV} must contain 64 hex characters"
        );
        for (index, pair) in bytes.chunks_exact(2).enumerate() {
            let high = decode_hex_byte(pair[0])
                .unwrap_or_else(|| panic!("{OTA_PUBLIC_KEY_ENV} contains a non-hex character"));
            let low = decode_hex_byte(pair[1])
                .unwrap_or_else(|| panic!("{OTA_PUBLIC_KEY_ENV} contains a non-hex character"));
            key[index] = (high << 4) | low;
        }
    }

    let key_literal = key
        .iter()
        .map(|byte| format!("0x{byte:02x}"))
        .collect::<Vec<_>>()
        .join(", ");
    let generated = format!(
        "pub(crate) const OTA_PUBLIC_KEY_CONFIGURED: bool = {};\n\
         #[allow(dead_code)]\n\
         pub(crate) const OTA_PUBLIC_KEY: [u8; 32] = [{key_literal}];\n\
         pub(crate) const OTA_BUILD_ID: &str = {build_id:?};\n",
        configured.is_some(),
    );
    let path = out_dir.join("ota_build_config.rs");
    fs::write(&path, generated)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
}

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

/// `config/linker/esp32/meditamer-memory.x` (our replacement for esp-hal's
/// own `memory.x`) `INCLUDE`s a `memory_extras.x` defining `RESERVE_DRAM` --
/// the DRAM `dram_seg` origin/length carve-out for the BT controller when it
/// is linked in. Through esp-hal 1.1.x, esp-hal's own build
/// script wrote that file (`generate_memory_extras`, gated on its internal
/// `__bluetooth` feature) into its own `OUT_DIR`, already on the linker
/// search path. esp-hal 1.2.0 inlined the same `RESERVE_DRAM` computation
/// directly into its *own* `memory.x` via a `#IF CARGO_FEATURE(...)`
/// preprocessor directive resolved by its own build script -- which our file
/// never goes through, since it replaces (not extends) esp-hal's memory.x.
/// So this crate reproduces the same on/off computation from the same
/// dependency edge that turns it on, but uses the exact ESP32 controller
/// footprint rather than esp-hal's conservative 64 KiB round-up. The pinned
/// `esp-wifi-sys-esp32` configuration declares `CONFIG_BTDM_RESERVE_DRAM =
/// 0xdb5c`, matching the highest fixed controller address (`SOC_MEM_BT_MISC_END
/// = 0x3ffbdb5c`) in esp-radio's ESP32 adapter.
///
/// `esp-radio`'s `ble` feature is the only
/// thing in this crate's graph that enables `esp-hal/__bluetooth`
/// (`vendor/esp-radio-1.0.0-beta.1-bounded/Cargo.toml`'s `ble = [
/// "esp-hal/__bluetooth", ... ]`), and only this crate's own
/// `ble-foundation` (direct `esp-radio/ble`) and `shared-ble-runtime` (via
/// `platform/connectivity/ble`'s unconditional `esp-radio/ble`) ever request it -- both
/// are this crate's own named features, so `CARGO_FEATURE_*` sees them
/// directly without needing esp-hal's own resolved feature state.
fn write_memory_extras(out_dir: &std::path::Path) {
    const BTDM_RESERVE_DRAM: usize = 0xdb5c;

    let bluetooth_linked = env::var_os("CARGO_FEATURE_BLE_FOUNDATION").is_some()
        || env::var_os("CARGO_FEATURE_SHARED_BLE_RUNTIME").is_some();
    let reserve_dram = if bluetooth_linked {
        BTDM_RESERVE_DRAM
    } else {
        0
    };
    let generated = format!(
        "/* generated by build.rs: exact ESP32 BTDM controller reservation */\n\
         RESERVE_DRAM = 0x{reserve_dram:x};\n"
    );
    let path = out_dir.join("memory_extras.x");
    fs::write(&path, generated)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_BLE_FOUNDATION");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SHARED_BLE_RUNTIME");
}

fn main() {
    if env::var_os("CARGO_FEATURE_CPU_LOAD").is_some() {
        for level in 1..=3 {
            println!("cargo:rustc-link-arg-bins=-Wl,--wrap=__level_{level}_interrupt");
        }
    }
    println!("cargo:rerun-if-env-changed=MEDITAMER_WIFI_LEGACY_LINK_WRAPS");
    if env_flag_enabled("MEDITAMER_WIFI_LEGACY_LINK_WRAPS") {
        emit_legacy_wifi_link_wrappers();
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR"));
    write_ota_build_config(&out_dir);
    write_memory_extras(&out_dir);
}
