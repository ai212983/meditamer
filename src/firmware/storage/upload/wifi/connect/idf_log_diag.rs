use super::*;

const WIFI_FIRST_START_IDF_LOG_DIAG: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_FIRST_START_IDF_LOG_DIAG") {
        Some(value) => Some(value),
        None => option_env!("WIFI_FIRST_START_IDF_LOG_DIAG"),
    },
);

static WIFI_FIRST_START_IDF_LOG_DIAG_ACTIVE: AtomicBool = AtomicBool::new(false);
static WIFI_FIRST_START_IDF_LOG_DIAG_ARMED: AtomicBool = AtomicBool::new(false);

fn wifi_log_level_debug() -> u32 {
    esp_wifi_sys::include::wifi_log_level_t_WIFI_LOG_DEBUG as u32
}

fn wifi_log_level_info() -> u32 {
    esp_wifi_sys::include::wifi_log_level_t_WIFI_LOG_INFO as u32
}

fn wifi_log_set(level: u32) -> i32 {
    unsafe { esp_wifi_sys::include::esp_wifi_internal_set_log_level(level as _) }
}

pub(super) fn maybe_begin_first_start_idf_log_diag() {
    if !WIFI_FIRST_START_IDF_LOG_DIAG
        || WIFI_FIRST_START_IDF_LOG_DIAG_ARMED.swap(true, Ordering::Relaxed)
    {
        return;
    }
    let target_level = wifi_log_level_debug();
    let set_rc = wifi_log_set(target_level);
    WIFI_FIRST_START_IDF_LOG_DIAG_ACTIVE.store(true, Ordering::Relaxed);
    diag_reassoc!(
        "upload_http: first_start_idf_log_diag set_rc={} target_level={}",
        set_rc,
        target_level,
    );
}

pub(super) fn maybe_end_first_start_idf_log_diag(reason: &str) {
    if !WIFI_FIRST_START_IDF_LOG_DIAG_ACTIVE.swap(false, Ordering::Relaxed) {
        return;
    }
    let restore_level = wifi_log_level_info();
    let set_rc = wifi_log_set(restore_level);
    diag_reassoc!(
        "upload_http: first_start_idf_log_diag_restore reason={} restore_level={} set_rc={}",
        reason,
        restore_level,
        set_rc,
    );
}
