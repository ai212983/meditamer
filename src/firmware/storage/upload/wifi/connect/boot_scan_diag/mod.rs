use super::blob_state_diag::log_blob_state_diag;
use super::boot_scan_idf_compare::{
    maybe_run_boot_scan_only_idf_explicit_compare, run_boot_scan_only_idf_null_compare,
};
use super::maybe_run_boot_scan_only_promisc_diag;
use super::*;

use core::sync::atomic::{AtomicBool, Ordering};

mod counters_core;
mod counters_runtime;
mod task_role;

use counters_core::log_boot_scan_only_core_counters;
use counters_runtime::log_boot_scan_only_runtime_counters;

const WIFI_BOOT_SCAN_ONLY_DIAG: bool =
    parse_nonzero_flag(match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG") {
        Some(value) => Some(value),
        None => option_env!("WIFI_BOOT_SCAN_ONLY_DIAG"),
    });
const WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST") {
        Some(value) => Some(value),
        None => option_env!("WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST"),
    },
);
const WIFI_BOOT_SCAN_ONLY_FORCE_WAKEUP_DIAG: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_FORCE_WAKEUP_DIAG") {
        Some(value) => Some(value),
        None => option_env!("WIFI_BOOT_SCAN_ONLY_FORCE_WAKEUP_DIAG"),
    },
);
const WIFI_BOOT_SCAN_ONLY_FORCE_PHY_ENABLE_DIAG: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_FORCE_PHY_ENABLE_DIAG") {
        Some(value) => Some(value),
        None => option_env!("WIFI_BOOT_SCAN_ONLY_FORCE_PHY_ENABLE_DIAG"),
    },
);
const WIFI_BOOT_SCAN_ONLY_DIAG_SCAN_TIMEOUT_MS: u64 = 15_000;
static WIFI_BOOT_SCAN_ONLY_DIAG_RAN: AtomicBool = AtomicBool::new(false);

pub(crate) const fn boot_scan_only_diag_enabled() -> bool {
    WIFI_BOOT_SCAN_ONLY_DIAG
}

fn log_boot_scan_only_diag_counters(stage: &str) {
    log_blob_state_diag(stage);
    log_boot_scan_only_core_counters(stage);
    log_boot_scan_only_runtime_counters(stage);
}

fn maybe_acquire_boot_scan_only_force_wakeup() -> bool {
    if !WIFI_BOOT_SCAN_ONLY_FORCE_WAKEUP_DIAG {
        return false;
    }
    let rc = unsafe { esp_wifi_sys::include::esp_wifi_force_wakeup_acquire() };
    println!(
        "upload_http: boot_scan_only_diag force_wakeup_acquire rc={}",
        rc
    );
    rc == 0
}

fn maybe_release_boot_scan_only_force_wakeup(acquired: bool) {
    if !acquired {
        return;
    }
    let rc = unsafe { esp_wifi_sys::include::esp_wifi_force_wakeup_release() };
    println!(
        "upload_http: boot_scan_only_diag force_wakeup_release rc={}",
        rc
    );
}

fn maybe_acquire_boot_scan_only_force_phy() -> bool {
    if !WIFI_BOOT_SCAN_ONLY_FORCE_PHY_ENABLE_DIAG {
        return false;
    }
    unsafe { esp_radio::diagnostic_wifi_phy_enable() };
    println!("upload_http: boot_scan_only_diag force_phy_enable invoked=true");
    true
}

fn maybe_release_boot_scan_only_force_phy(acquired: bool) {
    if !acquired {
        return;
    }
    unsafe { esp_radio::diagnostic_wifi_phy_disable() };
    println!("upload_http: boot_scan_only_diag force_phy_disable invoked=true");
}

pub(super) async fn maybe_run_boot_scan_only_diag(
    controller: &mut WifiController<'static>,
    credentials_present: bool,
) {
    if !WIFI_BOOT_SCAN_ONLY_DIAG || credentials_present {
        return;
    }
    if WIFI_BOOT_SCAN_ONLY_DIAG_RAN.swap(true, Ordering::Relaxed) {
        return;
    }

    esp_radio::diagnostic_reset_wifi_mac_isr_count();
    esp_radio::diagnostic_wifi_os_diag_reset();
    esp_radio::diagnostic_reset_phy_common_clock_diag();
    esp_radio::wifi::diagnostic_reset_wifi_rx_cb_counts();
    esp_radio::diagnostic_reset_wifi_scan_done_eventpost_diag();
    esp_radio::diagnostic_reset_timer_compat_diag();
    esp_radio::diagnostic_reset_timer_callback_exec_diag();
    esp_radio::diagnostic_reset_legacy_builtin_scheduler_diag();
    esp_rtos::diagnostic_esp_radio_sem_trace_reset();
    println!("upload_http: boot_scan_only_diag begin credentials_present=false");

    if let Err(err) = wifi_set_mode(controller, wifi_sta_mode()) {
        println!(
            "upload_http: boot_scan_only_diag outcome=set_mode_err err={:?}",
            err
        );
        return;
    }

    match with_timeout(
        Duration::from_millis(WIFI_DRIVER_CONTROL_TIMEOUT_MS),
        wifi_start_async(controller),
    )
    .await
    {
        Ok(Ok(())) => {
            println!("upload_http: boot_scan_only_diag start=ok");
        }
        Ok(Err(err)) => {
            println!(
                "upload_http: boot_scan_only_diag outcome=start_err err={:?}",
                err
            );
            return;
        }
        Err(_) => {
            println!(
                "upload_http: boot_scan_only_diag outcome=start_timeout timeout_ms={}",
                WIFI_DRIVER_CONTROL_TIMEOUT_MS
            );
            return;
        }
    }

    log_boot_scan_only_driver_state();
    log_blob_state_diag("after_start_ok");
    let force_wakeup_acquired = maybe_acquire_boot_scan_only_force_wakeup();
    let force_phy_acquired = maybe_acquire_boot_scan_only_force_phy();
    maybe_run_boot_scan_only_promisc_diag().await;

    if WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST {
        println!("upload_http: boot_scan_only_diag idf_null_first begin=true");
        run_boot_scan_only_idf_null_compare();
        log_boot_scan_only_diag_counters("idf_compare_first");
        if maybe_run_boot_scan_only_idf_explicit_compare() {
            log_boot_scan_only_diag_counters("idf_explicit_compare_first");
        }
    }

    let scan_started_at = Instant::now();
    let scan_config = driver::raw_broad_scan_config().with_max(WIFI_SCAN_DIAG_MAX_APS);
    match with_timeout(
        Duration::from_millis(WIFI_BOOT_SCAN_ONLY_DIAG_SCAN_TIMEOUT_MS),
        wifi_scan_with_config_async(controller, scan_config),
    )
    .await
    {
        Ok(Ok(results)) => {
            println!(
                "upload_http: boot_scan_only_diag scan=ok elapsed_ms={} result_count={}",
                elapsed_ms_u32(scan_started_at),
                results.len(),
            );
            log_boot_scan_only_diag_counters("rust_scan");
            for (idx, ap) in results.iter().take(10).enumerate() {
                println!(
                    "upload_http: boot_scan_only_diag ap idx={} ssid={} channel={} bssid={} rssi={} auth={:?}",
                    idx,
                    ap.ssid,
                    ap.channel,
                    format_bssid(ap.bssid),
                    ap.signal_strength,
                    ap.auth_method,
                );
            }
        }
        Ok(Err(err)) => {
            println!(
                "upload_http: boot_scan_only_diag outcome=scan_err elapsed_ms={} err={:?}",
                elapsed_ms_u32(scan_started_at),
                err
            );
            log_boot_scan_only_diag_counters("rust_scan_err");
        }
        Err(_) => {
            println!(
                "upload_http: boot_scan_only_diag outcome=scan_timeout timeout_ms={}",
                WIFI_BOOT_SCAN_ONLY_DIAG_SCAN_TIMEOUT_MS
            );
            log_boot_scan_only_diag_counters("rust_scan_timeout");
        }
    }

    if !WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST {
        run_boot_scan_only_idf_null_compare();
        log_boot_scan_only_diag_counters("idf_compare");
        if maybe_run_boot_scan_only_idf_explicit_compare() {
            log_boot_scan_only_diag_counters("idf_explicit_compare");
        }
    }

    maybe_release_boot_scan_only_force_phy(force_phy_acquired);
    maybe_release_boot_scan_only_force_wakeup(force_wakeup_acquired);

    match with_timeout(
        Duration::from_millis(WIFI_DRIVER_CONTROL_TIMEOUT_MS),
        wifi_stop_async(controller),
    )
    .await
    {
        Ok(Ok(())) => {
            println!("upload_http: boot_scan_only_diag stop=ok");
        }
        Ok(Err(err)) => {
            println!(
                "upload_http: boot_scan_only_diag outcome=stop_err err={:?}",
                err
            );
        }
        Err(_) => {
            println!(
                "upload_http: boot_scan_only_diag outcome=stop_timeout timeout_ms={}",
                WIFI_DRIVER_CONTROL_TIMEOUT_MS
            );
        }
    }
}
