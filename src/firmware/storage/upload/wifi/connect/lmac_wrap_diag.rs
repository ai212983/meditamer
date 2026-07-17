use core::sync::atomic::{AtomicUsize, Ordering};

use esp_println::println;

const SLOT_COUNT: usize = 8;

#[derive(Copy, Clone)]
struct Snapshot {
    arg0: u32,
    ret: u32,
    pre_mac_isr: u32,
    post_mac_isr: u32,
    pre_rx_sta: u32,
    post_rx_sta: u32,
}

impl Snapshot {
    const ZERO: Self = Self {
        arg0: 0,
        ret: 0,
        pre_mac_isr: 0,
        post_mac_isr: 0,
        pre_rx_sta: 0,
        post_rx_sta: 0,
    };
}

static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);
static SNAPSHOTS: [Snapshot; SLOT_COUNT] = [Snapshot::ZERO; SLOT_COUNT];

unsafe extern "C" {
    fn __real_lmacRxDone(a2: usize) -> usize;
}

pub(super) fn reset_lmac_wrap_diag() {
    NEXT_SLOT.store(0, Ordering::Relaxed);
    unsafe {
        core::ptr::write(
            SNAPSHOTS.as_ptr() as *mut [Snapshot; SLOT_COUNT],
            [Snapshot::ZERO; SLOT_COUNT],
        );
    }
}

pub(super) fn log_lmac_wrap_diag(stage: &str) {
    let count = NEXT_SLOT.load(Ordering::Relaxed).min(SLOT_COUNT);
    println!(
        "upload_http: boot_scan_only_diag lmac_rx_done_wrap_diag after={} count={}",
        stage, count
    );
    for idx in 0..count {
        let snap = unsafe { core::ptr::read(SNAPSHOTS.as_ptr().add(idx)) };
        println!(
            "upload_http: boot_scan_only_diag lmac_rx_done_wrap_diag_entry after={} idx={} arg0=0x{:08x} ret=0x{:08x} pre_mac_isr={} post_mac_isr={} pre_rx_sta={} post_rx_sta={}",
            stage,
            idx,
            snap.arg0,
            snap.ret,
            snap.pre_mac_isr,
            snap.post_mac_isr,
            snap.pre_rx_sta,
            snap.post_rx_sta,
        );
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_lmacRxDone(a2: usize) -> usize {
    let pre_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { __real_lmacRxDone(a2) };
    let post_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, _) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    let ordinal = NEXT_SLOT.fetch_add(1, Ordering::Relaxed);
    if ordinal < SLOT_COUNT {
        unsafe {
            core::ptr::write(
                SNAPSHOTS.as_ptr().add(ordinal) as *mut Snapshot,
                Snapshot {
                    arg0: a2 as u32,
                    ret: ret as u32,
                    pre_mac_isr,
                    post_mac_isr,
                    pre_rx_sta: pre_rx_sta as u32,
                    post_rx_sta: post_rx_sta as u32,
                },
            );
        }
    }
    ret
}
