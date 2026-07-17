use core::sync::atomic::{AtomicUsize, Ordering};

use esp_println::println;

const SLOT_COUNT: usize = 8;
const WORD_COUNT: usize = 4;

#[derive(Copy, Clone)]
struct Snapshot {
    args: [u32; 4],
    ret: u32,
    arg0_words: [u32; WORD_COUNT],
    arg2_words: [u32; WORD_COUNT],
    pre_mac_isr: u32,
    post_mac_isr: u32,
    pre_rx_sta: u32,
    post_rx_sta: u32,
}

impl Snapshot {
    const ZERO: Self = Self {
        args: [0; 4],
        ret: 0,
        arg0_words: [0; WORD_COUNT],
        arg2_words: [0; WORD_COUNT],
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

    fn print(&self, label: &str, phase: &str) {
        let count = self.next.load(Ordering::Relaxed).min(SLOT_COUNT);
        println!(
            "legacy_nostd_wifi_control: wdev_process_rx_wrap_diag label={} phase={} count={}",
            label, phase, count
        );
        for idx in 0..count {
            let snap = unsafe { core::ptr::read(self.snapshots.as_ptr().add(idx)) };
            println!(
                "legacy_nostd_wifi_control: wdev_process_rx_wrap_diag_entry label={} phase={} idx={} arg0=0x{:08x} arg1=0x{:08x} arg2=0x{:08x} arg3=0x{:08x} ret=0x{:08x} arg0_words={:08x}:{:08x}:{:08x}:{:08x} arg2_words={:08x}:{:08x}:{:08x}:{:08x} pre_mac_isr={} post_mac_isr={} pre_rx_sta={} post_rx_sta={}",
                label,
                phase,
                idx,
                snap.args[0],
                snap.args[1],
                snap.args[2],
                snap.args[3],
                snap.ret,
                snap.arg0_words[0],
                snap.arg0_words[1],
                snap.arg0_words[2],
                snap.arg0_words[3],
                snap.arg2_words[0],
                snap.arg2_words[1],
                snap.arg2_words[2],
                snap.arg2_words[3],
                snap.pre_mac_isr,
                snap.post_mac_isr,
                snap.pre_rx_sta,
                snap.post_rx_sta,
            );
        }
    }
}

fn capture_words(ptr: usize) -> [u32; WORD_COUNT] {
    let readable = (0x3f40_0000..0x4000_0000).contains(&ptr);
    if !readable {
        return [0; WORD_COUNT];
    }
    let mut out = [0u32; WORD_COUNT];
    let mut idx = 0usize;
    while idx < WORD_COUNT {
        out[idx] = unsafe { ((ptr + idx * 4) as *const u32).read_volatile() };
        idx += 1;
    }
    out
}

static RING: Ring = Ring::new();

unsafe extern "C" {
    fn __real_wDev_ProcessRxSucData(a2: usize, a3: usize, a4: usize, a5: usize) -> usize;
}

pub(crate) fn reset_wdev_process_rx_wrap_diag() {
    RING.reset();
}

pub(crate) fn print_wdev_process_rx_wrap_diag(label: &str, phase: &str) {
    RING.print(label, phase);
}

#[used]
static KEEP_WDEV_PROCESS_RX_WRAP: unsafe extern "C" fn(usize, usize, usize, usize) -> usize =
    __wrap_wDev_ProcessRxSucData;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wrap_wDev_ProcessRxSucData(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
) -> usize {
    let pre_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { __real_wDev_ProcessRxSucData(a2, a3, a4, a5) };
    let post_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    RING.push(Snapshot {
        args: [a2 as u32, a3 as u32, a4 as u32, a5 as u32],
        ret: ret as u32,
        arg0_words: capture_words(a2),
        arg2_words: capture_words(a4),
        pre_mac_isr,
        post_mac_isr,
        pre_rx_sta: pre_rx_sta as u32,
        post_rx_sta: post_rx_sta as u32,
    });
    ret
}
