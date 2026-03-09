use super::*;

const WIFI_SCAN_ENTRY_IDF_COMPARE_DIAG: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_SCAN_ENTRY_IDF_COMPARE_DIAG") {
        Some(value) => Some(value),
        None => option_env!("WIFI_SCAN_ENTRY_IDF_COMPARE_DIAG"),
    },
);
const WIFI_SCAN_ENTRY_IDF_COMPARE_MAX_RECORDS: usize = 16;

fn ssid_len(raw: &[u8; 33]) -> usize {
    raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len())
}

fn ssid_matches_target(raw: &[u8; 33], target: &str) -> bool {
    let len = ssid_len(raw);
    raw[..len] == *target.as_bytes()
}

pub(super) fn maybe_run_scan_entry_idf_compare_diag(target_ssid: &str) {
    if !WIFI_SCAN_ENTRY_IDF_COMPARE_DIAG || !telemetry::diag_enabled(DIAG_REASSOC) {
        return;
    }

    let scan_rc = unsafe { esp_wifi_sys::include::esp_wifi_scan_start(core::ptr::null(), true) };
    if scan_rc != esp_wifi_sys::include::ESP_OK as i32 {
        diag_reassoc!(
            "upload_http: scan_entry_idf_compare outcome=scan_start_err scan_rc={} target_ssid={}",
            scan_rc,
            target_ssid,
        );
        return;
    }

    let mut ap_num = 0u16;
    let ap_num_rc = unsafe { esp_wifi_sys::include::esp_wifi_scan_get_ap_num(&mut ap_num) };
    if ap_num_rc != esp_wifi_sys::include::ESP_OK as i32 {
        let _ = unsafe { esp_wifi_sys::include::esp_wifi_clear_ap_list() };
        diag_reassoc!(
            "upload_http: scan_entry_idf_compare outcome=get_ap_num_err scan_rc={} ap_num_rc={} target_ssid={}",
            scan_rc,
            ap_num_rc,
            target_ssid,
        );
        return;
    }

    let mut returned =
        core::cmp::min(ap_num as usize, WIFI_SCAN_ENTRY_IDF_COMPARE_MAX_RECORDS) as u16;
    let mut records = [unsafe { core::mem::zeroed::<esp_wifi_sys::include::wifi_ap_record_t>() };
        WIFI_SCAN_ENTRY_IDF_COMPARE_MAX_RECORDS];
    let records_rc = if returned == 0 {
        esp_wifi_sys::include::ESP_OK as i32
    } else {
        unsafe {
            esp_wifi_sys::include::esp_wifi_scan_get_ap_records(&mut returned, records.as_mut_ptr())
        }
    };
    if records_rc != esp_wifi_sys::include::ESP_OK as i32 {
        let _ = unsafe { esp_wifi_sys::include::esp_wifi_clear_ap_list() };
        diag_reassoc!(
            "upload_http: scan_entry_idf_compare outcome=get_ap_records_err scan_rc={} ap_num_rc={} records_rc={} ap_num={} target_ssid={}",
            scan_rc,
            ap_num_rc,
            records_rc,
            ap_num,
            target_ssid,
        );
        return;
    }

    let mut target_seen = false;
    let mut top_channel = 0u8;
    let mut top_bssid = None::<[u8; 6]>;
    let mut top_rssi = i8::MIN;
    for record in records.iter().take(returned as usize) {
        if ssid_matches_target(&record.ssid, target_ssid) {
            target_seen = true;
        }
        if record.rssi > top_rssi {
            top_rssi = record.rssi;
            top_channel = record.primary;
            top_bssid = Some(record.bssid);
        }
    }

    diag_reassoc!(
        "upload_http: scan_entry_idf_compare outcome=ok scan_rc={} ap_num_rc={} records_rc={} ap_num={} records_returned={} target_seen={} top_channel={} top_bssid={} top_rssi={}",
        scan_rc,
        ap_num_rc,
        records_rc,
        ap_num,
        returned,
        target_seen,
        top_channel,
        format_bssid_opt(top_bssid),
        top_rssi,
    );
}
