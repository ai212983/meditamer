use core::sync::atomic::{AtomicUsize, Ordering};

use esp_println::println;

const SLOT_COUNT: usize = 8;

#[derive(Copy, Clone)]
struct Snapshot {
    args: [u32; 4],
    ret: u32,
    pre_mac_isr: u32,
    post_mac_isr: u32,
    pre_rx_sta: u32,
    pre_rx_ap: u32,
    post_rx_sta: u32,
    post_rx_ap: u32,
}

impl Snapshot {
    const ZERO: Self = Self {
        args: [0; 4],
        ret: 0,
        pre_mac_isr: 0,
        post_mac_isr: 0,
        pre_rx_sta: 0,
        pre_rx_ap: 0,
        post_rx_sta: 0,
        post_rx_ap: 0,
    };
}

static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);
static mut SNAPSHOTS: [Snapshot; SLOT_COUNT] = [Snapshot::ZERO; SLOT_COUNT];

unsafe extern "C" {
    fn __real_sta_recv_mgmt(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize;
}

pub(super) fn reset_sta_recv_wrap_diag() {
    NEXT_SLOT.store(0, Ordering::Relaxed);
    unsafe {
        SNAPSHOTS = [Snapshot::ZERO; SLOT_COUNT];
    }
}

pub(super) fn log_sta_recv_wrap_diag(stage: &str) {
    let count = NEXT_SLOT.load(Ordering::Relaxed).min(SLOT_COUNT);
    println!(
        "upload_http: boot_scan_only_diag sta_recv_wrap_diag after={} count={}",
        stage, count
    );
    for idx in 0..count {
        let snap = unsafe { SNAPSHOTS[idx] };
        println!(
            "upload_http: boot_scan_only_diag sta_recv_wrap_diag_entry after={} idx={} arg0=0x{:08x} arg1=0x{:08x} arg2=0x{:08x} arg3=0x{:08x} ret=0x{:08x} pre_mac_isr={} post_mac_isr={} pre_rx_sta={} pre_rx_ap={} post_rx_sta={} post_rx_ap={}",
            stage,
            idx,
            snap.args[0],
            snap.args[1],
            snap.args[2],
            snap.args[3],
            snap.ret,
            snap.pre_mac_isr,
            snap.post_mac_isr,
            snap.pre_rx_sta,
            snap.pre_rx_ap,
            snap.post_rx_sta,
            snap.post_rx_ap,
        );
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_sta_recv_mgmt(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    let pre_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, pre_rx_ap) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { __real_sta_recv_mgmt(a2, a3, a4, a5, a6, a7) };
    let post_mac_isr = esp_radio::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, post_rx_ap) = esp_radio::wifi::diagnostic_wifi_rx_cb_counts();
    let ordinal = NEXT_SLOT.fetch_add(1, Ordering::Relaxed);
    if ordinal < SLOT_COUNT {
        unsafe {
            SNAPSHOTS[ordinal] = Snapshot {
                args: [a2 as u32, a3 as u32, a4 as u32, a5 as u32],
                ret: ret as u32,
                pre_mac_isr,
                post_mac_isr,
                pre_rx_sta: pre_rx_sta as u32,
                pre_rx_ap: pre_rx_ap as u32,
                post_rx_sta: post_rx_sta as u32,
                post_rx_ap: post_rx_ap as u32,
            };
        }
    }
    ret
}
