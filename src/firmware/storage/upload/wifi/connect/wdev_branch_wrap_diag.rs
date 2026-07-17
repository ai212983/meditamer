use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[path = "wdev_branch_wrap_diag/api.rs"]
mod api;
#[path = "wdev_branch_wrap_diag/rx.rs"]
mod rx;
#[path = "wdev_branch_wrap_diag/trampolines.rs"]
mod trampolines;
#[path = "wdev_branch_wrap_diag/wrappers.rs"]
mod wrappers;

const SLOT_COUNT: usize = 8;
const EVENT_WORDS: usize = 6;
const MAC_EVENT_WINDOW_BASE: usize = 0x3ff7_3c40;
const FORCE_EVENT_SEQ_LEN: usize = 8;
const FORCE_EVENT_SEQUENCE: [u32; FORCE_EVENT_SEQ_LEN] = [
    0x0100_0020,
    0x0080_0000,
    0x0000_0000,
    0x0000_0000,
    0x0000_0080,
    0x0000_0000,
    0x0100_0020,
    0x0080_0000,
];

const fn parse_nonzero_flag(value: Option<&'static str>) -> bool {
    match value {
        Some(raw) => {
            let bytes = raw.as_bytes();
            if bytes.is_empty() {
                false
            } else {
                !(bytes.len() == 1 && bytes[0] == b'0')
            }
        }
        None => false,
    }
}

const WIFI_BOOT_SCAN_ONLY_DIAG_FORCE_COMPARATOR_EVENT_SEQUENCE: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG_FORCE_COMPARATOR_EVENT_SEQUENCE") {
        Some(value) => Some(value),
        None => option_env!("WIFI_BOOT_SCAN_ONLY_DIAG_FORCE_COMPARATOR_EVENT_SEQUENCE"),
    },
);

#[derive(Copy, Clone)]
struct Snapshot {
    arg0: u32,
    arg1: u32,
    ret: u32,
    ret_forced: u32,
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
        ret_forced: 0,
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

#[derive(Copy, Clone)]
struct EventSnapshot {
    ret: u32,
    ret_forced: u32,
    pre_mac_isr: u32,
    post_mac_isr: u32,
    pre_words: [u32; EVENT_WORDS],
    post_words: [u32; EVENT_WORDS],
}

impl EventSnapshot {
    const ZERO: Self = Self {
        ret: 0,
        ret_forced: 0,
        pre_mac_isr: 0,
        post_mac_isr: 0,
        pre_words: [0; EVENT_WORDS],
        post_words: [0; EVENT_WORDS],
    };
}

struct EventRing {
    next: AtomicUsize,
    snapshots: [EventSnapshot; SLOT_COUNT],
}

impl EventRing {
    const fn new() -> Self {
        Self {
            next: AtomicUsize::new(0),
            snapshots: [EventSnapshot::ZERO; SLOT_COUNT],
        }
    }

    fn reset(&self) {
        self.next.store(0, Ordering::Relaxed);
        unsafe {
            core::ptr::write(
                self.snapshots.as_ptr() as *mut [EventSnapshot; SLOT_COUNT],
                [EventSnapshot::ZERO; SLOT_COUNT],
            );
        }
    }

    fn push(&self, snap: EventSnapshot) {
        let ordinal = self.next.fetch_add(1, Ordering::Relaxed);
        if ordinal < SLOT_COUNT {
            unsafe {
                core::ptr::write(self.snapshots.as_ptr().add(ordinal) as *mut EventSnapshot, snap);
            }
        }
    }

    fn log(&self, stage: &str, label: &str) {
        let count = self.next.load(Ordering::Relaxed).min(SLOT_COUNT);
        println!(
            "upload_http: boot_scan_only_diag {} after={} count={}",
            label, stage, count
        );
        for idx in 0..count {
            let snap = unsafe { core::ptr::read(self.snapshots.as_ptr().add(idx)) };
            println!(
                "upload_http: boot_scan_only_diag {}_entry after={} idx={} ret=0x{:08x} ret_forced=0x{:08x} pre_mac_isr={} post_mac_isr={} pre_words={:08x}:{:08x}:{:08x}:{:08x}:{:08x}:{:08x} post_words={:08x}:{:08x}:{:08x}:{:08x}:{:08x}:{:08x}",
                label,
                stage,
                idx,
                snap.ret,
                snap.ret_forced,
                snap.pre_mac_isr,
                snap.post_mac_isr,
                snap.pre_words[0],
                snap.pre_words[1],
                snap.pre_words[2],
                snap.pre_words[3],
                snap.pre_words[4],
                snap.pre_words[5],
                snap.post_words[0],
                snap.post_words[1],
                snap.post_words[2],
                snap.post_words[3],
                snap.post_words[4],
                snap.post_words[5],
            );
        }
    }
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

    fn log(&self, stage: &str, label: &str) {
        let count = self.next.load(Ordering::Relaxed).min(SLOT_COUNT);
        println!(
            "upload_http: boot_scan_only_diag {} after={} count={}",
            label, stage, count
        );
        for idx in 0..count {
            let snap = unsafe { core::ptr::read(self.snapshots.as_ptr().add(idx)) };
            println!(
                "upload_http: boot_scan_only_diag {}_entry after={} idx={} arg0=0x{:08x} arg1=0x{:08x} ret=0x{:08x} ret_forced=0x{:08x} pre_mac_isr={} post_mac_isr={} pre_rx_sta={} post_rx_sta={}",
                label,
                stage,
                idx,
                snap.arg0,
                snap.arg1,
                snap.ret,
                snap.ret_forced,
                snap.pre_mac_isr,
                snap.post_mac_isr,
                snap.pre_rx_sta,
                snap.post_rx_sta,
            );
        }
    }
}

static HAL_RX_END_RING: Ring = Ring::new();
static HAL_GET_EVENT_RING: Ring = Ring::new();
static HAL_CLR_EVENT_RING: Ring = Ring::new();
static HAL_GET_EVENT_EXT_RING: EventRing = EventRing::new();
static PANIC_WATCHDOG_RING: Ring = Ring::new();
static PP_POST_RING: Ring = Ring::new();
static LMAC_RX_SUC_RING: Ring = Ring::new();
static PP_POST_ARG25_COUNT: AtomicUsize = AtomicUsize::new(0);
static FORCE_EVENT_SEQ_NEXT: AtomicUsize = AtomicUsize::new(0);
static FORCE_EVENT_SEQ_ARMED: AtomicBool = AtomicBool::new(false);

pub(super) fn reset_wdev_branch_wrap_diag() {
    api::reset();
}

pub(super) fn set_force_comparator_event_sequence_diag_armed(armed: bool) {
    api::set_force_comparator_event_sequence_diag_armed(armed);
}

pub(super) fn log_wdev_branch_wrap_diag(stage: &str) {
    api::log(stage);
}

pub(super) fn log_wdev_binary_patch_counts(stage: &str) {
    api::log_binary_patch_counts(stage);
}
