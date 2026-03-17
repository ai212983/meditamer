use super::{blob_state_diag::log_blob_state_diag, *};

const WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE") {
        Some(value) => Some(value),
        None => option_env!("WIFI_BOOT_SCAN_ONLY_DIAG_IDF_EXPLICIT_COMPARE"),
    },
);
const WIFI_BOOT_SCAN_ONLY_DIAG_IDF_MAX_RECORDS: usize = 10;

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
    let mut ap_num = 0u16;
    let ap_num_rc = unsafe { esp_wifi_sys::include::esp_wifi_scan_get_ap_num(&mut ap_num) };
    if ap_num_rc != esp_wifi_sys::include::ESP_OK as i32 {
        let _ = unsafe { esp_wifi_sys::include::esp_wifi_clear_ap_list() };
        println!(
            "upload_http: boot_scan_only_diag {label}=get_ap_num_err scan_rc={} ap_num_rc={}",
            scan_rc, ap_num_rc
        );
        return;
    }

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
        let _ = unsafe { esp_wifi_sys::include::esp_wifi_clear_ap_list() };
        println!(
            "upload_http: boot_scan_only_diag {label}=get_ap_records_err scan_rc={} ap_num_rc={} records_rc={} ap_num={}",
            scan_rc, ap_num_rc, records_rc, ap_num
        );
        return;
    }

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
        show_hidden: true,
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

    println!(
        "upload_http: boot_scan_only_diag idf_explicit_compare begin=true active_min_ms={} active_max_ms={} passive_ms=0 home_chan_dwell_ms={} show_hidden=true channel=0",
        active_min_ms, active_max_ms, home_chan_dwell_time
    );
    log_blob_state_diag("idf_explicit_compare_prestart");
    let scan_rc = unsafe { esp_wifi_sys::include::esp_wifi_scan_start(&scan_config, true) };
    log_idf_explicit_postcall_diag("idf_explicit_compare_postcall", scan_rc);
    if scan_rc != esp_wifi_sys::include::ESP_OK as i32 {
        println!(
            "upload_http: boot_scan_only_diag idf_explicit_compare=scan_start_err scan_rc={}",
            scan_rc
        );
        return true;
    }
    log_idf_scan_results("idf_explicit_compare", scan_rc);
    true
}
