use super::recovery::disconnect_and_force_deep_reinit_with_timeout;
use super::*;

const WIFI_SCAN_ENTRY_PROMISC_DIAG: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_SCAN_ENTRY_PROMISC_DIAG") {
        Some(value) => Some(value),
        None => option_env!("WIFI_SCAN_ENTRY_PROMISC_DIAG"),
    },
);
const WIFI_POST_START_PROMISC_DIAG: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_POST_START_PROMISC_DIAG") {
        Some(value) => Some(value),
        None => option_env!("WIFI_POST_START_PROMISC_DIAG"),
    },
);
const WIFI_SOFTWARE_RESET_ON_POST_START_PROMISC_ZERO: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_SOFTWARE_RESET_ON_POST_START_PROMISC_ZERO") {
        Some(value) => Some(value),
        None => option_env!("WIFI_SOFTWARE_RESET_ON_POST_START_PROMISC_ZERO"),
    },
);
const WIFI_POST_START_PROMISC_ZERO_HARD_REINIT: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_POST_START_PROMISC_ZERO_HARD_REINIT") {
        Some(value) => Some(value),
        None => option_env!("WIFI_POST_START_PROMISC_ZERO_HARD_REINIT"),
    },
);
const WIFI_BOOT_SCAN_ONLY_PROMISC_DIAG: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_PROMISC_DIAG") {
        Some(value) => Some(value),
        None => option_env!("WIFI_BOOT_SCAN_ONLY_PROMISC_DIAG"),
    },
);
const WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS_DEFAULT: u64 = 120;
const WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS_MIN: u64 = 50;
const WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS_MAX: u64 = 3_000;
const WIFI_SCAN_ENTRY_PROMISC_DIAG_CHANNELS: [u8; 4] = [8, 1, 6, 11];
const WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS: u64 = {
    let configured = match option_env!("MEDITAMER_WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS") {
        Some(value) => Some(value),
        None => option_env!("WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS"),
    };
    match configured {
        Some(raw) => match parse_ascii_u64(raw) {
            Some(value)
                if value >= WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS_MIN
                    && value <= WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS_MAX =>
            {
                value
            }
            _ => WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS_DEFAULT,
        },
        None => WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS_DEFAULT,
    }
};

static PROMISC_PKT_TOTAL: AtomicU32 = AtomicU32::new(0);
static PROMISC_PKT_MGMT: AtomicU32 = AtomicU32::new(0);
static PROMISC_PKT_CTRL: AtomicU32 = AtomicU32::new(0);
static PROMISC_PKT_DATA: AtomicU32 = AtomicU32::new(0);
static PROMISC_PKT_MISC: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn promisc_rx_cb(
    _buf: *mut esp_wifi_sys::c_types::c_void,
    pkt_type: esp_wifi_sys::include::wifi_promiscuous_pkt_type_t,
) {
    PROMISC_PKT_TOTAL.fetch_add(1, Ordering::Relaxed);
    match pkt_type {
        esp_wifi_sys::include::wifi_promiscuous_pkt_type_t_WIFI_PKT_MGMT => {
            PROMISC_PKT_MGMT.fetch_add(1, Ordering::Relaxed);
        }
        esp_wifi_sys::include::wifi_promiscuous_pkt_type_t_WIFI_PKT_CTRL => {
            PROMISC_PKT_CTRL.fetch_add(1, Ordering::Relaxed);
        }
        esp_wifi_sys::include::wifi_promiscuous_pkt_type_t_WIFI_PKT_DATA => {
            PROMISC_PKT_DATA.fetch_add(1, Ordering::Relaxed);
        }
        esp_wifi_sys::include::wifi_promiscuous_pkt_type_t_WIFI_PKT_MISC => {
            PROMISC_PKT_MISC.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
}

fn reset_promisc_counters() {
    PROMISC_PKT_TOTAL.store(0, Ordering::Relaxed);
    PROMISC_PKT_MGMT.store(0, Ordering::Relaxed);
    PROMISC_PKT_CTRL.store(0, Ordering::Relaxed);
    PROMISC_PKT_DATA.store(0, Ordering::Relaxed);
    PROMISC_PKT_MISC.store(0, Ordering::Relaxed);
}

fn promisc_totals() -> (u32, u32, u32, u32, u32) {
    (
        PROMISC_PKT_TOTAL.load(Ordering::Relaxed),
        PROMISC_PKT_MGMT.load(Ordering::Relaxed),
        PROMISC_PKT_CTRL.load(Ordering::Relaxed),
        PROMISC_PKT_DATA.load(Ordering::Relaxed),
        PROMISC_PKT_MISC.load(Ordering::Relaxed),
    )
}

async fn run_promisc_diag(label: &'static str, software_reset_on_zero: bool) -> Option<u32> {
    if !telemetry::diag_enabled(DIAG_REASSOC) {
        return None;
    }

    let mut was_enabled = false;
    let get_before_rc =
        unsafe { esp_wifi_sys::include::esp_wifi_get_promiscuous(&mut was_enabled as *mut bool) };
    if get_before_rc != esp_wifi_sys::include::ESP_OK as i32 {
        diag_reassoc!(
            "upload_http: {} outcome=get_before_err get_before_rc={}",
            label,
            get_before_rc,
        );
        return None;
    }
    if was_enabled {
        diag_reassoc!("upload_http: {} outcome=skip_already_enabled", label);
        return None;
    }

    let cb_rc =
        unsafe { esp_wifi_sys::include::esp_wifi_set_promiscuous_rx_cb(Some(promisc_rx_cb)) };
    let filter = esp_wifi_sys::include::wifi_promiscuous_filter_t {
        filter_mask: esp_wifi_sys::include::WIFI_PROMIS_FILTER_MASK_MGMT
            | esp_wifi_sys::include::WIFI_PROMIS_FILTER_MASK_CTRL
            | esp_wifi_sys::include::WIFI_PROMIS_FILTER_MASK_DATA,
    };
    let filter_rc = unsafe { esp_wifi_sys::include::esp_wifi_set_promiscuous_filter(&filter) };
    let enable_rc = unsafe { esp_wifi_sys::include::esp_wifi_set_promiscuous(true) };
    if cb_rc != esp_wifi_sys::include::ESP_OK as i32
        || filter_rc != esp_wifi_sys::include::ESP_OK as i32
        || enable_rc != esp_wifi_sys::include::ESP_OK as i32
    {
        let disable_rc = unsafe { esp_wifi_sys::include::esp_wifi_set_promiscuous(false) };
        let clear_cb_rc = unsafe { esp_wifi_sys::include::esp_wifi_set_promiscuous_rx_cb(None) };
        diag_reassoc!(
            "upload_http: {} outcome=enable_err get_before_rc={} cb_rc={} filter_rc={} enable_rc={} disable_rc={} clear_cb_rc={}",
            label,
            get_before_rc,
            cb_rc,
            filter_rc,
            enable_rc,
            disable_rc,
            clear_cb_rc,
        );
        return None;
    }

    let mut orig_primary = 0u8;
    let mut orig_second = esp_wifi_sys::include::wifi_second_chan_t_WIFI_SECOND_CHAN_NONE;
    let get_channel_rc = unsafe {
        esp_wifi_sys::include::esp_wifi_get_channel(
            &mut orig_primary as *mut u8,
            &mut orig_second as *mut esp_wifi_sys::include::wifi_second_chan_t,
        )
    };

    let mut aggregate_total = 0u32;
    let mut aggregate_mgmt = 0u32;
    let mut aggregate_ctrl = 0u32;
    let mut aggregate_data = 0u32;
    let mut aggregate_misc = 0u32;
    for channel in WIFI_SCAN_ENTRY_PROMISC_DIAG_CHANNELS {
        let set_channel_rc = unsafe {
            esp_wifi_sys::include::esp_wifi_set_channel(
                channel,
                esp_wifi_sys::include::wifi_second_chan_t_WIFI_SECOND_CHAN_NONE,
            )
        };
        reset_promisc_counters();
        Timer::after(Duration::from_millis(WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS)).await;
        let (total, mgmt, ctrl, data, misc) = promisc_totals();
        aggregate_total = aggregate_total.saturating_add(total);
        aggregate_mgmt = aggregate_mgmt.saturating_add(mgmt);
        aggregate_ctrl = aggregate_ctrl.saturating_add(ctrl);
        aggregate_data = aggregate_data.saturating_add(data);
        aggregate_misc = aggregate_misc.saturating_add(misc);
        diag_reassoc!(
            "upload_http: {} window channel={} dwell_ms={} set_channel_rc={} total={} mgmt={} ctrl={} data={} misc={}",
            label,
            channel,
            WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS,
            set_channel_rc,
            total,
            mgmt,
            ctrl,
            data,
            misc,
        );
    }

    let restore_channel_rc = if get_channel_rc == esp_wifi_sys::include::ESP_OK as i32 {
        unsafe { esp_wifi_sys::include::esp_wifi_set_channel(orig_primary, orig_second) }
    } else {
        esp_wifi_sys::include::ESP_FAIL as i32
    };

    let disable_rc = unsafe { esp_wifi_sys::include::esp_wifi_set_promiscuous(false) };
    let clear_cb_rc = unsafe { esp_wifi_sys::include::esp_wifi_set_promiscuous_rx_cb(None) };
    let mut enabled_after = false;
    let get_after_rc =
        unsafe { esp_wifi_sys::include::esp_wifi_get_promiscuous(&mut enabled_after as *mut bool) };

    diag_reassoc!(
        "upload_http: {} outcome=ok channels={:?} dwell_ms={} get_before_rc={} cb_rc={} filter_rc={} enable_rc={} get_channel_rc={} restore_channel_rc={} disable_rc={} clear_cb_rc={} get_after_rc={} enabled_after={} total={} mgmt={} ctrl={} data={} misc={}",
        label,
        WIFI_SCAN_ENTRY_PROMISC_DIAG_CHANNELS,
        WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS,
        get_before_rc,
        cb_rc,
        filter_rc,
        enable_rc,
        get_channel_rc,
        restore_channel_rc,
        disable_rc,
        clear_cb_rc,
        get_after_rc,
        enabled_after,
        aggregate_total,
        aggregate_mgmt,
        aggregate_ctrl,
        aggregate_data,
        aggregate_misc,
    );
    if software_reset_on_zero && aggregate_total == 0 {
        diag_reassoc!(
            "upload_http: {} software_reset=true reason=post_start_promisc_zero total={} channels={:?} dwell_ms={}",
            label,
            aggregate_total,
            WIFI_SCAN_ENTRY_PROMISC_DIAG_CHANNELS,
            WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS,
        );
        Timer::after(Duration::from_millis(250)).await;
        esp_hal::system::software_reset();
    }
    Some(aggregate_total)
}

pub(super) async fn maybe_handle_post_start_promisc_diag(
    controller: &mut WifiController<'static>,
    state: &mut WifiTaskState,
) -> bool {
    if !WIFI_POST_START_PROMISC_DIAG {
        return false;
    }
    let aggregate_total = run_promisc_diag(
        "post_start_promisc_diag",
        WIFI_SOFTWARE_RESET_ON_POST_START_PROMISC_ZERO,
    )
    .await;
    if !WIFI_POST_START_PROMISC_ZERO_HARD_REINIT || aggregate_total != Some(0) {
        return false;
    }

    diag_reassoc!(
        "upload_http: post_start_promisc_zero_hard_reinit trigger=true start_ok_age_ms={} start_attempt_age_ms={} auth_idx={} channel_hint={:?} bssid_hint={}",
        WifiTaskState::point_age_ms(state.start_ok_at),
        WifiTaskState::point_age_ms(state.start_attempt_started_at),
        state.auth_method_idx,
        state.channel_hint,
        format_bssid_opt(state.bssid_hint),
    );
    state.config_applied = false;
    state.failure_class = NetFailureClass::DiscoveryEmpty;
    state.failure_code = WIFI_REASON_NO_AP_FOUND;
    state.ladder_step = RecoveryLadderStep::DriverRestart;
    state.channel_hint = None;
    state.bssid_hint = None;
    state.ap_candidates.clear();
    state.ap_candidate_idx = 0;
    state.channel_probe_idx = 0;
    state.force_full_channel_probe_next_scan = true;
    transition_state(
        &mut state.net_state,
        NetState::Recovering,
        "post_start_promisc_zero_hard_reinit",
        state.started_at,
        state.ladder_step,
        state.net_attempt,
        (state.failure_class, state.failure_code),
    );
    publish_state(
        state.net_state,
        state.ladder_step,
        state.net_attempt,
        state.failure_class,
        state.failure_code,
        state.started_at.elapsed().as_millis() as u32,
    );
    state.start_hard_recover_watchdog("post_start_promisc_zero_hard_reinit");
    disconnect_and_force_deep_reinit_with_timeout(
        controller,
        "post_start_promisc_zero_hard_reinit",
    )
    .await;
    true
}

pub(super) async fn maybe_run_scan_entry_promisc_diag() {
    if WIFI_SCAN_ENTRY_PROMISC_DIAG {
        let _ = run_promisc_diag("scan_entry_promisc_diag", false).await;
    }
}

pub(super) async fn maybe_run_boot_scan_only_promisc_diag() {
    if WIFI_BOOT_SCAN_ONLY_PROMISC_DIAG {
        let _ = run_promisc_diag("boot_scan_only_promisc_diag", false).await;
    }
}
