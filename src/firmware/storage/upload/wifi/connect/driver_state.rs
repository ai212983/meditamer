use super::*;

const WIFI_SCAN_ENTRY_DRIVER_STATE_DIAG: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_SCAN_ENTRY_DRIVER_STATE_DIAG") {
        Some(value) => Some(value),
        None => option_env!("WIFI_SCAN_ENTRY_DRIVER_STATE_DIAG"),
    },
);
const WIFI_EARLY_DRIVER_STATE_DIAG: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_EARLY_DRIVER_STATE_DIAG") {
        Some(value) => Some(value),
        None => option_env!("WIFI_EARLY_DRIVER_STATE_DIAG"),
    },
);
static WIFI_FIRST_START_DRIVER_STATE_DIAG_EMITTED: AtomicBool = AtomicBool::new(false);
static WIFI_PRE_START_DRIVER_STATE_DIAG_EMITTED: AtomicBool = AtomicBool::new(false);

fn cc_char(byte: u8) -> char {
    if byte.is_ascii_graphic() {
        byte as char
    } else {
        '.'
    }
}

fn log_driver_state(label: &str, force: bool) {
    if !force && !telemetry::diag_enabled(DIAG_REASSOC) {
        return;
    }

    let mut mode = 0u32;
    let mode_rc = unsafe { esp_wifi_sys::include::esp_wifi_get_mode(&mut mode as *mut u32) };
    let mut channel_primary = 0u8;
    let mut channel_second = 0u32;
    let channel_rc = unsafe {
        esp_wifi_sys::include::esp_wifi_get_channel(
            &mut channel_primary as *mut u8,
            &mut channel_second as *mut u32,
        )
    };
    let mut ps = 0u32;
    let ps_rc = unsafe { esp_wifi_sys::include::esp_wifi_get_ps(&mut ps as *mut u32) };
    let mut max_tx_power = 0i8;
    let max_tx_power_rc =
        unsafe { esp_wifi_sys::include::esp_wifi_get_max_tx_power(&mut max_tx_power as *mut i8) };
    let mut event_mask = 0u32;
    let event_mask_rc =
        unsafe { esp_wifi_sys::include::esp_wifi_get_event_mask(&mut event_mask as *mut u32) };
    let mut protocol_bitmap = 0u8;
    let protocol_rc = unsafe {
        esp_wifi_sys::include::esp_wifi_get_protocol(
            esp_wifi_sys::include::wifi_interface_t_WIFI_IF_STA,
            &mut protocol_bitmap as *mut u8,
        )
    };
    let mut scan_defaults =
        core::mem::MaybeUninit::<esp_wifi_sys::include::wifi_scan_default_params_t>::uninit();
    let scan_defaults_rc =
        unsafe { esp_wifi_sys::include::esp_wifi_get_scan_parameters(scan_defaults.as_mut_ptr()) };
    let mut country = core::mem::MaybeUninit::<esp_wifi_sys::include::wifi_country_t>::uninit();
    let country_rc = unsafe { esp_wifi_sys::include::esp_wifi_get_country(country.as_mut_ptr()) };

    let country_fields = if country_rc == esp_wifi_sys::include::ESP_OK as i32 {
        let country = unsafe { country.assume_init() };
        (
            cc_char(country.cc[0]),
            cc_char(country.cc[1]),
            cc_char(country.cc[2]),
            country.schan,
            country.nchan,
            country.max_tx_power,
            country.policy,
        )
    } else {
        ('.', '.', '.', 0, 0, 0, 0)
    };
    let scan_defaults_fields = if scan_defaults_rc == esp_wifi_sys::include::ESP_OK as i32 {
        let scan_defaults = unsafe { scan_defaults.assume_init() };
        (
            scan_defaults.scan_time.active.min,
            scan_defaults.scan_time.active.max,
            scan_defaults.scan_time.passive,
            scan_defaults.home_chan_dwell_time,
        )
    } else {
        (0, 0, 0, 0)
    };

    if force {
        println!(
            "upload_http: {} mode_rc={} mode={} channel_rc={} primary={} second={} ps_rc={} ps={} max_tx_power_rc={} max_tx_power={} event_mask_rc={} event_mask=0x{:08x} protocol_rc={} protocol_bitmap=0x{:02x} country_rc={} cc={}{}{} schan={} nchan={} country_max_tx_power={} policy={} scan_defaults_rc={} scan_active_min={} scan_active_max={} scan_passive={} scan_home_dwell={}",
            label,
            mode_rc,
            mode,
            channel_rc,
            channel_primary,
            channel_second,
            ps_rc,
            ps,
            max_tx_power_rc,
            max_tx_power,
            event_mask_rc,
            event_mask,
            protocol_rc,
            protocol_bitmap,
            country_rc,
            country_fields.0,
            country_fields.1,
            country_fields.2,
            country_fields.3,
            country_fields.4,
            country_fields.5,
            country_fields.6,
            scan_defaults_rc,
            scan_defaults_fields.0,
            scan_defaults_fields.1,
            scan_defaults_fields.2,
            scan_defaults_fields.3,
        );
    } else {
        diag_reassoc!(
            "upload_http: {} mode_rc={} mode={} channel_rc={} primary={} second={} ps_rc={} ps={} max_tx_power_rc={} max_tx_power={} event_mask_rc={} event_mask=0x{:08x} protocol_rc={} protocol_bitmap=0x{:02x} country_rc={} cc={}{}{} schan={} nchan={} country_max_tx_power={} policy={} scan_defaults_rc={} scan_active_min={} scan_active_max={} scan_passive={} scan_home_dwell={}",
            label,
            mode_rc,
            mode,
            channel_rc,
            channel_primary,
            channel_second,
            ps_rc,
            ps,
            max_tx_power_rc,
            max_tx_power,
            event_mask_rc,
            event_mask,
            protocol_rc,
            protocol_bitmap,
            country_rc,
            country_fields.0,
            country_fields.1,
            country_fields.2,
            country_fields.3,
            country_fields.4,
            country_fields.5,
            country_fields.6,
            scan_defaults_rc,
            scan_defaults_fields.0,
            scan_defaults_fields.1,
            scan_defaults_fields.2,
            scan_defaults_fields.3,
        );
    }
}

fn first_nul_len(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len())
}

fn log_sta_config(
    label: &str,
    expected_auth_idx: usize,
    expected_channel_hint: Option<u8>,
    expected_bssid_hint: Option<[u8; 6]>,
) {
    let mut config = core::mem::MaybeUninit::<esp_wifi_sys::include::wifi_config_t>::zeroed();
    let config_rc = unsafe {
        esp_wifi_sys::include::esp_wifi_get_config(
            esp_wifi_sys::include::wifi_interface_t_WIFI_IF_STA,
            config.as_mut_ptr(),
        )
    };
    if config_rc != esp_wifi_sys::include::ESP_OK as i32 {
        diag_reassoc!(
            "upload_http: {} config_rc={} expected_auth_idx={} expected_channel_hint={:?} expected_bssid_hint={}",
            label,
            config_rc,
            expected_auth_idx,
            expected_channel_hint,
            format_bssid_opt(expected_bssid_hint),
        );
        return;
    }

    let sta = unsafe { config.assume_init().sta };
    diag_reassoc!(
        "upload_http: {} config_rc={} ssid_len={} scan_method={} bssid_set={} bssid={} channel={} listen_interval={} sort_method={} threshold_authmode={} threshold_rssi={} failure_retry_cnt={} expected_auth_idx={} expected_channel_hint={:?} expected_bssid_hint={}",
        label,
        config_rc,
        first_nul_len(&sta.ssid),
        sta.scan_method,
        sta.bssid_set,
        format_bssid(sta.bssid),
        sta.channel,
        sta.listen_interval,
        sta.sort_method,
        sta.threshold.authmode,
        sta.threshold.rssi,
        sta.failure_retry_cnt,
        expected_auth_idx,
        expected_channel_hint,
        format_bssid_opt(expected_bssid_hint),
    );
}

pub(super) fn maybe_log_scan_entry_driver_state() {
    if WIFI_SCAN_ENTRY_DRIVER_STATE_DIAG {
        log_driver_state("scan_entry_driver_state", false);
    }
}

pub(super) fn log_boot_scan_only_driver_state() {
    log_driver_state("boot_scan_only_driver_state", true);
}

pub(super) fn maybe_log_first_start_driver_state(
    expected_auth_idx: usize,
    expected_channel_hint: Option<u8>,
    expected_bssid_hint: Option<[u8; 6]>,
) {
    if !WIFI_EARLY_DRIVER_STATE_DIAG
        || WIFI_FIRST_START_DRIVER_STATE_DIAG_EMITTED.swap(true, Ordering::Relaxed)
    {
        return;
    }
    maybe_begin_first_start_idf_log_diag();
    log_driver_state("first_start_driver_state", true);
    log_sta_config(
        "first_start_sta_config",
        expected_auth_idx,
        expected_channel_hint,
        expected_bssid_hint,
    );
}

pub(super) fn maybe_log_pre_start_driver_state(
    expected_auth_idx: usize,
    expected_channel_hint: Option<u8>,
    expected_bssid_hint: Option<[u8; 6]>,
) {
    if !WIFI_EARLY_DRIVER_STATE_DIAG
        || WIFI_PRE_START_DRIVER_STATE_DIAG_EMITTED.swap(true, Ordering::Relaxed)
    {
        return;
    }
    log_driver_state("pre_start_driver_state", true);
    log_sta_config(
        "pre_start_sta_config",
        expected_auth_idx,
        expected_channel_hint,
        expected_bssid_hint,
    );
}
