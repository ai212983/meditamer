#[cfg(wifi_sniffer_passthrough_diag)]
use core::sync::atomic::{AtomicUsize, Ordering};

#[cfg(wifi_sniffer_passthrough_diag)]
use esp_println::println;

#[cfg(wifi_sniffer_passthrough_diag)]
static SNIFFER_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(wifi_sniffer_passthrough_diag)]
static SNIFFER_LAST_A10: AtomicUsize = AtomicUsize::new(0);
#[cfg(wifi_sniffer_passthrough_diag)]
static SNIFFER_LAST_A11: AtomicUsize = AtomicUsize::new(0);
#[cfg(wifi_sniffer_passthrough_diag)]
static SNIFFER_LAST_META48: AtomicUsize = AtomicUsize::new(0);
#[cfg(wifi_sniffer_passthrough_diag)]
static SNIFFER_LAST_META52: AtomicUsize = AtomicUsize::new(0);

#[cfg(wifi_sniffer_passthrough_diag)]
unsafe extern "C" {
    #[link_name = "wDev_SnifferRxData"]
    fn wdev_sniffer_rxdata_real(a10: usize, a11: usize) -> usize;
}

#[cfg(wifi_sniffer_passthrough_diag)]
#[no_mangle]
pub unsafe extern "C" fn wdev_sniffer_passthrough(a10: usize, a11: usize) -> usize {
    SNIFFER_COUNT.fetch_add(1, Ordering::Relaxed);
    SNIFFER_LAST_A10.store(a10, Ordering::Relaxed);
    SNIFFER_LAST_A11.store(a11, Ordering::Relaxed);
    if a10 != 0 {
        let meta48 = core::ptr::read_unaligned((a10 as *const u8).add(48)) as usize;
        let meta52 = core::ptr::read_unaligned((a10 as *const u32).add(13)) as usize;
        SNIFFER_LAST_META48.store(meta48, Ordering::Relaxed);
        SNIFFER_LAST_META52.store(meta52, Ordering::Relaxed);
    }
    unsafe { wdev_sniffer_rxdata_real(a10, a11) }
}

#[cfg(wifi_sniffer_passthrough_diag)]
pub(super) fn reset_wdev_sniffer_wrap_diag() {
    SNIFFER_COUNT.store(0, Ordering::Relaxed);
    SNIFFER_LAST_A10.store(0, Ordering::Relaxed);
    SNIFFER_LAST_A11.store(0, Ordering::Relaxed);
    SNIFFER_LAST_META48.store(0, Ordering::Relaxed);
    SNIFFER_LAST_META52.store(0, Ordering::Relaxed);
}

#[cfg(wifi_sniffer_passthrough_diag)]
pub(super) fn log_wdev_sniffer_wrap_diag(stage: &str) {
    let count = SNIFFER_COUNT.load(Ordering::Relaxed);
    let a10 = SNIFFER_LAST_A10.load(Ordering::Relaxed);
    let a11 = SNIFFER_LAST_A11.load(Ordering::Relaxed);
    let meta48 = SNIFFER_LAST_META48.load(Ordering::Relaxed);
    let meta52 = SNIFFER_LAST_META52.load(Ordering::Relaxed);
    println!(
        "upload_http: boot_scan_only_diag wdev_sniffer_wrap_diag after={} count={} a10=0x{:08x} a11=0x{:08x} meta48=0x{:02x} meta52=0x{:08x}",
        stage, count, a10, a11, meta48, meta52
    );
}

#[cfg(not(wifi_sniffer_passthrough_diag))]
pub(super) fn reset_wdev_sniffer_wrap_diag() {}

#[cfg(not(wifi_sniffer_passthrough_diag))]
pub(super) fn log_wdev_sniffer_wrap_diag(_stage: &str) {}
