use core::sync::atomic::{AtomicUsize, Ordering};

use esp_println::println;

const SLOT_COUNT: usize = 8;

#[derive(Copy, Clone)]
struct Snapshot {
    arg0: u32,
    arg1: u32,
    ret: u32,
    pre_mac_isr: u32,
    post_mac_isr: u32,
    pre_rx_sta: u32,
    post_rx_sta: u32,
}

impl Snapshot {
    const ZERO: Self = Self {
        arg0: 0,
        arg1: 0,
        ret: 0,
        pre_mac_isr: 0,
        post_mac_isr: 0,
        pre_rx_sta: 0,
        post_rx_sta: 0,
    };
}

struct Ring {
    next: AtomicUsize,
    snapshots: [Snapshot; SLOT_COUNT],
}

impl Ring {
    const fn new() -> Self {
        Self {
            next: AtomicUsize::new(0),
            snapshots: [Snapshot::ZERO; SLOT_COUNT],
        }
    }

    fn reset(&self) {
        self.next.store(0, Ordering::Relaxed);
        unsafe {
            core::ptr::write(
                self.snapshots.as_ptr() as *mut [Snapshot; SLOT_COUNT],
                [Snapshot::ZERO; SLOT_COUNT],
            );
        }
    }

    fn push(&self, snap: Snapshot) {
        let ordinal = self.next.fetch_add(1, Ordering::Relaxed);
        if ordinal < SLOT_COUNT {
            unsafe {
                core::ptr::write(self.snapshots.as_ptr().add(ordinal) as *mut Snapshot, snap);
            }
        }
    }

    fn print(&self, label: &str, phase: &str, ring_label: &str) {
        let count = self.next.load(Ordering::Relaxed).min(SLOT_COUNT);
        println!(
            "legacy_nostd_wifi_control: {} label={} phase={} count={}",
            ring_label, label, phase, count
        );
        for idx in 0..count {
            let snap = unsafe { core::ptr::read(self.snapshots.as_ptr().add(idx)) };
            println!(
                "legacy_nostd_wifi_control: {}_entry label={} phase={} idx={} arg0=0x{:08x} arg1=0x{:08x} ret=0x{:08x} pre_mac_isr={} post_mac_isr={} pre_rx_sta={} post_rx_sta={}",
                ring_label,
                label,
                phase,
                idx,
                snap.arg0,
                snap.arg1,
                snap.ret,
                snap.pre_mac_isr,
                snap.post_mac_isr,
                snap.pre_rx_sta,
                snap.post_rx_sta,
            );
        }
    }
}

static GET_EVENT_RING: Ring = Ring::new();
static CLR_EVENT_RING: Ring = Ring::new();
static RX_END_RING: Ring = Ring::new();
static PP_POST_RING: Ring = Ring::new();
static LMAC_RX_SUC_RING: Ring = Ring::new();
static LMAC_RX_DONE_RING: Ring = Ring::new();
static PPENQ_RING: Ring = Ring::new();

unsafe extern "C" {
    fn __real_hal_mac_interrupt_get_event() -> usize;
    fn __real_hal_mac_interrupt_clr_event(a2: usize) -> usize;
    fn __real_hal_mac_rx_get_end_info(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_pp_post(a2: usize, a3: usize) -> usize;
    fn __real_lmacProcessRxSucData() -> usize;
    fn __real_lmacRxDone(a2: usize) -> usize;
    fn __real_ppEnqueueRxq(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
}

fn snapshot() -> (u32, u32, u32, u32) {
    let pre_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    (pre_mac_isr, pre_rx_sta as u32, 0, 0)
}

pub(crate) fn reset_wdev_branch_wrap_diag() {
    GET_EVENT_RING.reset();
    CLR_EVENT_RING.reset();
    RX_END_RING.reset();
    PP_POST_RING.reset();
    LMAC_RX_SUC_RING.reset();
    LMAC_RX_DONE_RING.reset();
    PPENQ_RING.reset();
}

pub(crate) fn print_wdev_branch_wrap_diag(label: &str, phase: &str) {
    GET_EVENT_RING.print(label, phase, "hal_mac_get_event_wrap_diag");
    CLR_EVENT_RING.print(label, phase, "hal_mac_clr_event_wrap_diag");
    RX_END_RING.print(label, phase, "hal_mac_rx_end_wrap_diag");
    PP_POST_RING.print(label, phase, "pp_post_wrap_diag");
    LMAC_RX_SUC_RING.print(label, phase, "lmac_rx_suc_wrap_diag");
    LMAC_RX_DONE_RING.print(label, phase, "lmac_rx_done_wrap_diag");
    PPENQ_RING.print(label, phase, "ppenq_wrap_diag");
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_hal_mac_interrupt_get_event() -> usize {
    let pre_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { __real_hal_mac_interrupt_get_event() };
    let post_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    GET_EVENT_RING.push(Snapshot {
        arg0: 0,
        arg1: 0,
        ret: ret as u32,
        pre_mac_isr,
        post_mac_isr,
        pre_rx_sta: pre_rx_sta as u32,
        post_rx_sta: post_rx_sta as u32,
    });
    ret
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_hal_mac_interrupt_clr_event(a2: usize) -> usize {
    let pre_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { __real_hal_mac_interrupt_clr_event(a2) };
    let post_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    CLR_EVENT_RING.push(Snapshot {
        arg0: a2 as u32,
        arg1: 0,
        ret: ret as u32,
        pre_mac_isr,
        post_mac_isr,
        pre_rx_sta: pre_rx_sta as u32,
        post_rx_sta: post_rx_sta as u32,
    });
    ret
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_hal_mac_rx_get_end_info(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    let pre_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { __real_hal_mac_rx_get_end_info(a2, a3, a4, a5, a6, a7) };
    let post_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    RX_END_RING.push(Snapshot {
        arg0: a2 as u32,
        arg1: a3 as u32,
        ret: ret as u32,
        pre_mac_isr,
        post_mac_isr,
        pre_rx_sta: pre_rx_sta as u32,
        post_rx_sta: post_rx_sta as u32,
    });
    ret
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_pp_post(a2: usize, a3: usize) -> usize {
    let pre_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { __real_pp_post(a2, a3) };
    let post_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    PP_POST_RING.push(Snapshot {
        arg0: a2 as u32,
        arg1: a3 as u32,
        ret: ret as u32,
        pre_mac_isr,
        post_mac_isr,
        pre_rx_sta: pre_rx_sta as u32,
        post_rx_sta: post_rx_sta as u32,
    });
    ret
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_lmacProcessRxSucData() -> usize {
    let pre_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { __real_lmacProcessRxSucData() };
    let post_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    LMAC_RX_SUC_RING.push(Snapshot {
        arg0: 0,
        arg1: 0,
        ret: ret as u32,
        pre_mac_isr,
        post_mac_isr,
        pre_rx_sta: pre_rx_sta as u32,
        post_rx_sta: post_rx_sta as u32,
    });
    ret
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_lmacRxDone(a2: usize) -> usize {
    let pre_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { __real_lmacRxDone(a2) };
    let post_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    LMAC_RX_DONE_RING.push(Snapshot {
        arg0: a2 as u32,
        arg1: 0,
        ret: ret as u32,
        pre_mac_isr,
        post_mac_isr,
        pre_rx_sta: pre_rx_sta as u32,
        post_rx_sta: post_rx_sta as u32,
    });
    ret
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ppEnqueueRxq(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    let pre_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { __real_ppEnqueueRxq(a2, a3, a4, a5, a6, a7) };
    let post_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    PPENQ_RING.push(Snapshot {
        arg0: a2 as u32,
        arg1: a3 as u32,
        ret: ret as u32,
        pre_mac_isr,
        post_mac_isr,
        pre_rx_sta: pre_rx_sta as u32,
        post_rx_sta: post_rx_sta as u32,
    });
    ret
}
