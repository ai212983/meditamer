use super::*;

#[derive(Copy, Clone)]
pub(super) struct RxDirectSnapshot {
    pub(super) args: [u32; 4],
    pub(super) ret: u32,
    pub(super) pre_mac_isr: u32,
    pub(super) post_mac_isr: u32,
    pub(super) pre_rx_sta: u32,
    pub(super) post_rx_sta: u32,
}

impl RxDirectSnapshot {
    const ZERO: Self = Self {
        args: [0; 4],
        ret: 0,
        pre_mac_isr: 0,
        post_mac_isr: 0,
        pre_rx_sta: 0,
        post_rx_sta: 0,
    };
}

pub(super) struct RxDirectRing {
    next: AtomicUsize,
    snapshots: [RxDirectSnapshot; SLOT_COUNT],
}

impl RxDirectRing {
    const fn new() -> Self {
        Self {
            next: AtomicUsize::new(0),
            snapshots: [RxDirectSnapshot::ZERO; SLOT_COUNT],
        }
    }

    pub(super) fn reset(&self) {
        self.next.store(0, Ordering::Relaxed);
        unsafe {
            core::ptr::write(
                self.snapshots.as_ptr() as *mut [RxDirectSnapshot; SLOT_COUNT],
                [RxDirectSnapshot::ZERO; SLOT_COUNT],
            );
        }
    }

    pub(super) fn push(&self, snap: RxDirectSnapshot) {
        let ordinal = self.next.fetch_add(1, Ordering::Relaxed);
        if ordinal < SLOT_COUNT {
            unsafe {
                core::ptr::write(
                    self.snapshots.as_ptr().add(ordinal) as *mut RxDirectSnapshot,
                    snap,
                );
            }
        }
    }

    pub(super) fn log(&self, stage: &str) {
        let count = self.next.load(Ordering::Relaxed).min(SLOT_COUNT);
        println!(
            "upload_http: boot_scan_only_diag wdev_process_rx_binary_wrap_diag after={} count={}",
            stage, count
        );
        for idx in 0..count {
            let snap = unsafe { core::ptr::read(self.snapshots.as_ptr().add(idx)) };
            println!(
                "upload_http: boot_scan_only_diag wdev_process_rx_binary_wrap_diag_entry after={} idx={} arg0=0x{:08x} arg1=0x{:08x} arg2=0x{:08x} arg3=0x{:08x} ret=0x{:08x} pre_mac_isr={} post_mac_isr={} pre_rx_sta={} post_rx_sta={}",
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
                snap.post_rx_sta,
            );
        }
    }
}

pub(super) static WDEV_PROCESS_RX_DIRECT_RING: RxDirectRing = RxDirectRing::new();
