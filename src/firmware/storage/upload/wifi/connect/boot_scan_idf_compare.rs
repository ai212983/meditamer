use super::{
    blob_state_diag::{
        log_blob_state_diag, log_scan_list_probe_diag, log_scan_prelink_summary_diag,
    },
    wdev_branch_wrap_diag::set_force_comparator_event_sequence_diag_armed,
    *,
};

const WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE") {
        Some(value) => Some(value),
        None => option_env!("WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE"),
    },
);
const WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPAT071: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPAT071") {
        Some(value) => Some(value),
        None => option_env!("WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPAT071"),
    },
);
const WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_SHOW_HIDDEN: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_SHOW_HIDDEN") {
        Some(value) => Some(value),
        None => match option_env!("WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_SHOW_HIDDEN") {
            Some(value) => Some(value),
            None => Some("1"),
        },
    },
);
const WIFI_BOOT_SCAN_ONLY_DIAG_MAC_EVENT_W1_OR_COMPARATOR_BITS: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_MAC_EVENT_W1_OR_COMPARATOR_BITS") {
        Some(value) => Some(value),
        None => option_env!("WIFI_BOOT_SCAN_ONLY_DIAG_MAC_EVENT_W1_OR_COMPARATOR_BITS"),
    },
);
const WIFI_BOOT_SCAN_ONLY_DIAG_IDF_MAX_RECORDS: usize = 10;
const MAC_EVENT_W1_ADDR: usize = 0x3ff73c44;
const MAC_EVENT_W1_COMPARATOR_DELTA: u32 = 0x0200_0200;

#[repr(C)]
#[derive(Copy, Clone)]
struct WifiScanConfigCompat071 {
    ssid: *mut u8,
    bssid: *mut u8,
    channel: u8,
    show_hidden: bool,
    scan_type: esp_wifi_sys::include::wifi_scan_type_t,
    scan_time: esp_wifi_sys::include::wifi_scan_time_t,
    home_chan_dwell_time: u8,
    channel_bitmap: esp_wifi_sys::include::wifi_scan_channel_bitmap_t,
}

fn log_idf_explicit_postcall_diag(stage: &str, scan_rc: i32) {
    let (rx_sta, rx_ap) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    let os_diag = esp_radio::diagnostic_wifi_os_diag_snapshot();
    let scan_done = esp_radio::diagnostic_wifi_scan_done_eventpost_diag();
    let adapter_diag = esp_radio::diagnostic_wifi_adapter_primitive_diag();
    println!(
        "upload_http: boot_scan_only_diag {stage}=postcall scan_rc={} wifi_mac_isr_count={} rx_sta={} rx_ap={} queue_send={} queue_send_isr={} queue_recv={} event_post={} scan_done_count={} scan_done_status={} scan_done_ap_num={} thread_sem_get={} task_get_current_task_count={}",
        scan_rc,
        esp_radio::diagnostic_wifi_mac_isr_count(),
        rx_sta,
        rx_ap,
        os_diag.queue_send,
        os_diag.queue_send_isr,
        os_diag.queue_recv,
        os_diag.event_post,
        scan_done.count,
        scan_done.status,
        scan_done.ap_num,
        adapter_diag.thread_sem_get_count,
        adapter_diag.task_get_current_task_count,
    );
    log_blob_state_diag(stage);
    super::boot_scan_diag::log_boot_scan_only_diag_counters_external(stage);
    super::super::backend_legacy_port::log_runtime_state(stage);
}

const fn wifi_use_idf_default_scan_timing_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_IDF_DEFAULT_SCAN_TIMING_DIAG"),
        Some(_)
    ) || matches!(
        option_env!("ESP_RADIO_USE_IDF_DEFAULT_SCAN_TIMING_DIAG"),
        Some(_)
    )
}

fn log_idf_scan_results(label: &str, scan_rc: i32) {
    log_scan_list_probe_diag(label, "before_get_ap_num");
    log_scan_prelink_summary_diag(label, "before_get_ap_num");
    let mut ap_num = 0u16;
    let ap_num_rc = unsafe { esp_wifi_sys::include::esp_wifi_scan_get_ap_num(&mut ap_num) };
    if ap_num_rc != esp_wifi_sys::include::ESP_OK as i32 {
        log_scan_list_probe_diag(label, "get_ap_num_err");
        log_scan_prelink_summary_diag(label, "get_ap_num_err");
        let _ = unsafe { esp_wifi_sys::include::esp_wifi_clear_ap_list() };
        log_scan_list_probe_diag(label, "after_clear_ap_list");
        log_scan_prelink_summary_diag(label, "after_clear_ap_list");
        println!(
            "upload_http: boot_scan_only_diag {label}=get_ap_num_err scan_rc={} ap_num_rc={}",
            scan_rc, ap_num_rc
        );
        return;
    }
    log_scan_list_probe_diag(label, "after_get_ap_num");
    log_scan_prelink_summary_diag(label, "after_get_ap_num");

    let mut returned =
        core::cmp::min(ap_num as usize, WIFI_BOOT_SCAN_ONLY_DIAG_IDF_MAX_RECORDS) as u16;
    let mut records = [unsafe { core::mem::zeroed::<esp_wifi_sys::include::wifi_ap_record_t>() };
        WIFI_BOOT_SCAN_ONLY_DIAG_IDF_MAX_RECORDS];
    let records_rc = if returned == 0 {
        esp_wifi_sys::include::ESP_OK as i32
    } else {
        unsafe {
            esp_wifi_sys::include::esp_wifi_scan_get_ap_records(&mut returned, records.as_mut_ptr())
        }
    };
    if records_rc != esp_wifi_sys::include::ESP_OK as i32 {
        log_scan_list_probe_diag(label, "get_ap_records_err");
        log_scan_prelink_summary_diag(label, "get_ap_records_err");
        let _ = unsafe { esp_wifi_sys::include::esp_wifi_clear_ap_list() };
        log_scan_list_probe_diag(label, "after_clear_ap_list");
        log_scan_prelink_summary_diag(label, "after_clear_ap_list");
        println!(
            "upload_http: boot_scan_only_diag {label}=get_ap_records_err scan_rc={} ap_num_rc={} records_rc={} ap_num={}",
            scan_rc, ap_num_rc, records_rc, ap_num
        );
        return;
    }
    log_scan_list_probe_diag(label, "after_get_ap_records");
    log_scan_prelink_summary_diag(label, "after_get_ap_records");

    println!(
        "upload_http: boot_scan_only_diag {label}=ok scan_rc={} ap_num_rc={} records_rc={} ap_num={} records_returned={}",
        scan_rc, ap_num_rc, records_rc, ap_num, returned
    );
    for (idx, record) in records.iter().take(returned as usize).enumerate() {
        let ssid_len = record
            .ssid
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(record.ssid.len());
        let ssid = core::str::from_utf8(&record.ssid[..ssid_len]).unwrap_or("<non_utf8>");
        println!(
            "upload_http: boot_scan_only_diag {label}_ap idx={} ssid={} channel={} bssid={} rssi={} auth={}",
            idx,
            ssid,
            record.primary,
            format_bssid(record.bssid),
            record.rssi,
            record.authmode
        );
    }
}

fn maybe_apply_mac_event_w1_or_comparator_bits_diag() {
    if !WIFI_BOOT_SCAN_ONLY_DIAG_MAC_EVENT_W1_OR_COMPARATOR_BITS {
        return;
    }
    let ptr = MAC_EVENT_W1_ADDR as *mut u32;
    let before = unsafe { ptr.read_volatile() };
    let after = before | MAC_EVENT_W1_COMPARATOR_DELTA;
    unsafe { ptr.write_volatile(after) };
    println!(
        "upload_http: boot_scan_only_diag mac_event_w1_or_diag addr=0x{:08x} before=0x{:08x} mask=0x{:08x} after=0x{:08x}",
        MAC_EVENT_W1_ADDR as u32,
        before,
        MAC_EVENT_W1_COMPARATOR_DELTA,
        after,
    );
    log_blob_state_diag("idf_explicit_compare_prestart_forced");
}

pub(super) fn run_boot_scan_only_idf_null_compare() {
    let scan_rc = unsafe { esp_wifi_sys::include::esp_wifi_scan_start(core::ptr::null(), true) };
    if scan_rc != esp_wifi_sys::include::ESP_OK as i32 {
        println!(
            "upload_http: boot_scan_only_diag idf_compare=scan_start_err scan_rc={}",
            scan_rc
        );
        return;
    }
    log_idf_scan_results("idf_compare", scan_rc);
}

pub(super) fn maybe_run_boot_scan_only_idf_explicit_compare() -> bool {
    if !WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE {
        return false;
    }

    let (active_min_ms, active_max_ms, home_chan_dwell_time) =
        if wifi_use_idf_default_scan_timing_diag_enabled() {
            (0, 120, 30)
        } else {
            (10, 20, 0)
        };

    let scan_config = esp_wifi_sys::include::wifi_scan_config_t {
        ssid: core::ptr::null_mut(),
        bssid: core::ptr::null_mut(),
        channel: 0,
        show_hidden: WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_SHOW_HIDDEN,
        scan_type: esp_wifi_sys::include::wifi_scan_type_t_WIFI_SCAN_TYPE_ACTIVE,
        scan_time: esp_wifi_sys::include::wifi_scan_time_t {
            active: esp_wifi_sys::include::wifi_active_scan_time_t {
                min: active_min_ms,
                max: active_max_ms,
            },
            passive: 0,
        },
        home_chan_dwell_time,
        channel_bitmap: esp_wifi_sys::include::wifi_scan_channel_bitmap_t {
            ghz_2_channels: 0,
            ghz_5_channels: 0,
        },
        coex_background_scan: false,
    };
    let scan_config_compat = WifiScanConfigCompat071 {
        ssid: core::ptr::null_mut(),
        bssid: core::ptr::null_mut(),
        channel: 0,
        show_hidden: WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_SHOW_HIDDEN,
        scan_type: esp_wifi_sys::include::wifi_scan_type_t_WIFI_SCAN_TYPE_ACTIVE,
        scan_time: esp_wifi_sys::include::wifi_scan_time_t {
            active: esp_wifi_sys::include::wifi_active_scan_time_t {
                min: active_min_ms,
                max: active_max_ms,
            },
            passive: 0,
        },
        home_chan_dwell_time,
        channel_bitmap: esp_wifi_sys::include::wifi_scan_channel_bitmap_t {
            ghz_2_channels: 0,
            ghz_5_channels: 0,
        },
    };
    let use_compat071 = WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPAT071;

    println!(
        "upload_http: boot_scan_only_diag idf_explicit_compare begin=true compat071={} active_min_ms={} active_max_ms={} passive_ms=0 home_chan_dwell_ms={} show_hidden={} channel=0 size_current={} size_compat071={}",
        use_compat071 as u8,
        active_min_ms,
        active_max_ms,
        home_chan_dwell_time,
        WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_SHOW_HIDDEN as u8,
        core::mem::size_of::<esp_wifi_sys::include::wifi_scan_config_t>(),
        core::mem::size_of::<WifiScanConfigCompat071>(),
    );
    log_blob_state_diag("idf_explicit_compare_prestart");
    maybe_apply_mac_event_w1_or_comparator_bits_diag();
    set_force_comparator_event_sequence_diag_armed(true);
    let scan_rc = unsafe {
        if use_compat071 {
            esp_wifi_sys::include::esp_wifi_scan_start(
                (&scan_config_compat as *const WifiScanConfigCompat071).cast(),
                true,
            )
        } else {
            esp_wifi_sys::include::esp_wifi_scan_start(&scan_config, true)
        }
    };
    log_idf_explicit_postcall_diag("idf_explicit_compare_postcall", scan_rc);
    if scan_rc != esp_wifi_sys::include::ESP_OK as i32 {
        set_force_comparator_event_sequence_diag_armed(false);
        println!(
            "upload_http: boot_scan_only_diag idf_explicit_compare=scan_start_err scan_rc={}",
            scan_rc
        );
        return true;
    }
    log_idf_scan_results("idf_explicit_compare", scan_rc);
    set_force_comparator_event_sequence_diag_armed(false);
    true
}
