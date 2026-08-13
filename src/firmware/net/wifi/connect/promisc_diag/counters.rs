use core::sync::atomic::{AtomicU32, Ordering};

static PROMISC_PKT_TOTAL: AtomicU32 = AtomicU32::new(0);
static PROMISC_PKT_MGMT: AtomicU32 = AtomicU32::new(0);
static PROMISC_PKT_CTRL: AtomicU32 = AtomicU32::new(0);
static PROMISC_PKT_DATA: AtomicU32 = AtomicU32::new(0);
static PROMISC_PKT_MISC: AtomicU32 = AtomicU32::new(0);

pub(super) unsafe extern "C" fn promisc_rx_cb(
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

pub(super) fn reset_promisc_counters() {
    PROMISC_PKT_TOTAL.store(0, Ordering::Relaxed);
    PROMISC_PKT_MGMT.store(0, Ordering::Relaxed);
    PROMISC_PKT_CTRL.store(0, Ordering::Relaxed);
    PROMISC_PKT_DATA.store(0, Ordering::Relaxed);
    PROMISC_PKT_MISC.store(0, Ordering::Relaxed);
}

pub(super) fn promisc_totals() -> (u32, u32, u32, u32, u32) {
    (
        PROMISC_PKT_TOTAL.load(Ordering::Relaxed),
        PROMISC_PKT_MGMT.load(Ordering::Relaxed),
        PROMISC_PKT_CTRL.load(Ordering::Relaxed),
        PROMISC_PKT_DATA.load(Ordering::Relaxed),
        PROMISC_PKT_MISC.load(Ordering::Relaxed),
    )
}
