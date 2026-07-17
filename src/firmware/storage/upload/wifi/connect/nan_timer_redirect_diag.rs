use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use esp_println::println;

use esp_wifi_sys::c_types::c_void;

fn nan_timer_redirect_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_NAN_TO_TIMER_PROCESS_REDIRECT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_NAN_TO_TIMER_PROCESS_REDIRECT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

static REDIRECT_COUNT: AtomicU32 = AtomicU32::new(0);
static PASSTHROUGH_COUNT: AtomicU32 = AtomicU32::new(0);
static LAST_ARG_PTR: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" {
    fn __real_nan_dp_schedule_ndc_start(arg: *mut c_void);
    fn ieee80211_timer_process(arg: *mut c_void);
}

pub(super) fn reset_nan_timer_redirect_diag() {
    REDIRECT_COUNT.store(0, Ordering::Relaxed);
    PASSTHROUGH_COUNT.store(0, Ordering::Relaxed);
    LAST_ARG_PTR.store(0, Ordering::Relaxed);
}

pub(super) fn log_nan_timer_redirect_diag(stage: &str) {
    println!(
        "upload_http: boot_scan_only_diag nan_timer_redirect_diag after={} enabled={} redirect_count={} passthrough_count={} last_arg_ptr=0x{:x}",
        stage,
        nan_timer_redirect_enabled(),
        REDIRECT_COUNT.load(Ordering::Relaxed),
        PASSTHROUGH_COUNT.load(Ordering::Relaxed),
        LAST_ARG_PTR.load(Ordering::Relaxed),
    );
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_nan_dp_schedule_ndc_start(arg: *mut c_void) {
    LAST_ARG_PTR.store(arg as usize, Ordering::Relaxed);
    if nan_timer_redirect_enabled() {
        REDIRECT_COUNT.fetch_add(1, Ordering::Relaxed);
        unsafe { ieee80211_timer_process(arg) };
    } else {
        PASSTHROUGH_COUNT.fetch_add(1, Ordering::Relaxed);
        unsafe { __real_nan_dp_schedule_ndc_start(arg) };
    }
}
