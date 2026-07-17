use super::super::legacy_discovery;
use super::blob_state_diag::log_blob_state_diag;
use super::boot_scan_idf_compare::{
    maybe_run_boot_scan_only_idf_explicit_compare, run_boot_scan_only_idf_null_compare,
};
use super::maybe_run_boot_scan_only_promisc_diag;
use super::nan_timer_slot_retarget_diag::maybe_apply_nan_timer_slot_retarget_diag;
use super::scan_cmd_helper_wrap_diag::reset_scan_cmd_helper_wrap_diag;
use super::*;

use core::sync::atomic::{AtomicBool, Ordering};
use embassy_time::Timer;

mod counters_core;
mod counters_preempt;
mod counters_runtime;
mod support;
mod task_role;

use counters_core::log_boot_scan_only_core_counters;
use counters_preempt::log_legacy_preempt_builtin;
use counters_runtime::log_boot_scan_only_runtime_counters;
use support::{
    boot_scan_only_diag_idf_explicit_first_delay_ms, log_boot_scan_only_diag_counters,
    reset_boot_scan_only_diag_counters,
};

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
const WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_FIRST: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_FIRST") {
        Some(value) => Some(value),
        None => option_env!("WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_FIRST"),
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

pub(super) fn log_boot_scan_only_diag_counters_external(stage: &str) {
    log_boot_scan_only_diag_counters(stage);
}

async fn log_boot_scan_only_diag_counters_settled(stage: &'static str) {
    Timer::after(Duration::from_millis(50)).await;
    log_boot_scan_only_diag_counters(stage);
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

    log_boot_scan_only_diag_counters("before_diag_reset");
    reset_boot_scan_only_diag_counters();
    println!("upload_http: boot_scan_only_diag begin credentials_present=false");

    if let Err(err) = wifi_set_mode(controller, wifi_sta_mode()) {
        println!(
            "upload_http: boot_scan_only_diag outcome=set_mode_err err={:?}",
            err
        );
        return;
    }

    log_boot_scan_only_diag_counters("after_set_mode");

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

    log_boot_scan_only_diag_counters("after_start_pre_driver_state");
    maybe_apply_nan_timer_slot_retarget_diag();
    log_boot_scan_only_diag_counters("after_nan_timer_slot_retarget");
    log_boot_scan_only_driver_state();
    log_blob_state_diag("after_start_ok");
    let force_wakeup_acquired = maybe_acquire_boot_scan_only_force_wakeup();
    let force_phy_acquired = maybe_acquire_boot_scan_only_force_phy();
    maybe_run_boot_scan_only_promisc_diag().await;
    let mut explicit_compare_ran = false;

    if WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_FIRST {
        reset_scan_cmd_helper_wrap_diag();
    }
    if WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_FIRST
        && maybe_run_boot_scan_only_idf_explicit_compare()
    {
        let explicit_first_delay_ms = boot_scan_only_diag_idf_explicit_first_delay_ms();
        if explicit_first_delay_ms != 0 {
            println!(
                "upload_http: boot_scan_only_diag idf_explicit_prefirst_delay_ms={}",
                explicit_first_delay_ms
            );
            Timer::after(Duration::from_millis(explicit_first_delay_ms)).await;
        }
        explicit_compare_ran = true;
        log_boot_scan_only_diag_counters("idf_explicit_compare_prefirst");
        Timer::after(Duration::from_millis(100)).await;
        log_boot_scan_only_runtime_counters("idf_explicit_compare_prefirst_runtime_delayed");
        log_boot_scan_only_diag_counters_settled("idf_explicit_compare_prefirst_settled").await;
    }

    if WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST {
        println!("upload_http: boot_scan_only_diag idf_null_first begin=true");
        run_boot_scan_only_idf_null_compare();
        log_boot_scan_only_diag_counters("idf_compare_first");
        log_boot_scan_only_diag_counters_settled("idf_compare_first_settled").await;
        if !explicit_compare_ran {
            reset_scan_cmd_helper_wrap_diag();
        }
        if !explicit_compare_ran && maybe_run_boot_scan_only_idf_explicit_compare() {
            log_boot_scan_only_diag_counters("idf_explicit_compare_first");
            log_boot_scan_only_diag_counters_settled("idf_explicit_compare_first_settled").await;
        }
    }

    let scan_started_at = Instant::now();
    match with_timeout(
        Duration::from_millis(WIFI_BOOT_SCAN_ONLY_DIAG_SCAN_TIMEOUT_MS),
        async {
            let mut session = legacy_discovery::begin_session(controller).await?;
            let results =
                legacy_discovery::scan_broad(&mut session, WIFI_SCAN_DIAG_MAX_APS).await?;
            legacy_discovery::shutdown(session).await?;
            Ok::<_, WifiError>(results)
        },
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
            log_boot_scan_only_diag_counters_settled("rust_scan_settled").await;
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
            log_boot_scan_only_diag_counters_settled("rust_scan_err_settled").await;
        }
        Err(_) => {
            println!(
                "upload_http: boot_scan_only_diag outcome=scan_timeout timeout_ms={}",
                WIFI_BOOT_SCAN_ONLY_DIAG_SCAN_TIMEOUT_MS
            );
            log_boot_scan_only_diag_counters("rust_scan_timeout");
            log_boot_scan_only_diag_counters_settled("rust_scan_timeout_settled").await;
        }
    }

    if !WIFI_BOOT_SCAN_ONLY_DIAG_IDF_NULL_FIRST {
        run_boot_scan_only_idf_null_compare();
        log_boot_scan_only_diag_counters("idf_compare");
        log_boot_scan_only_diag_counters_settled("idf_compare_settled").await;
        if !explicit_compare_ran {
            reset_scan_cmd_helper_wrap_diag();
        }
        if !explicit_compare_ran && maybe_run_boot_scan_only_idf_explicit_compare() {
            log_boot_scan_only_diag_counters("idf_explicit_compare");
            log_boot_scan_only_diag_counters_settled("idf_explicit_compare_settled").await;
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
