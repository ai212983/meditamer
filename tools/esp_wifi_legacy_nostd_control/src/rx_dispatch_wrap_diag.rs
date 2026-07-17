use core::sync::atomic::{AtomicUsize, Ordering};

use esp_println::println;

const SLOT_COUNT: usize = 8;
const WORD_COUNT: usize = 4;

#[derive(Copy, Clone)]
struct Snapshot {
    args: [u32; 4],
    ret: u32,
    arg3_words: [u32; WORD_COUNT],
    ret_words: [u32; WORD_COUNT],
    pre_mac_isr: u32,
    post_mac_isr: u32,
    pre_rx_sta: u32,
    post_rx_sta: u32,
}

impl Snapshot {
    const ZERO: Self = Self {
        args: [0; 4],
        ret: 0,
        arg3_words: [0; WORD_COUNT],
        ret_words: [0; WORD_COUNT],
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
                "legacy_nostd_wifi_control: {}_entry label={} phase={} idx={} arg0=0x{:08x} arg1=0x{:08x} arg2=0x{:08x} arg3=0x{:08x} ret=0x{:08x} arg3_words={:08x}:{:08x}:{:08x}:{:08x} ret_words={:08x}:{:08x}:{:08x}:{:08x} pre_mac_isr={} post_mac_isr={} pre_rx_sta={} post_rx_sta={}",
                ring_label,
                label,
                phase,
                idx,
                snap.args[0],
                snap.args[1],
                snap.args[2],
                snap.args[3],
                snap.ret,
                snap.arg3_words[0],
                snap.arg3_words[1],
                snap.arg3_words[2],
                snap.arg3_words[3],
                snap.ret_words[0],
                snap.ret_words[1],
                snap.ret_words[2],
                snap.ret_words[3],
                snap.pre_mac_isr,
                snap.post_mac_isr,
                snap.pre_rx_sta,
                snap.post_rx_sta,
            );
        }
    }
}

fn capture_words(ptr: usize) -> [u32; WORD_COUNT] {
    let readable = (0x3f40_0000..0x4000_0000).contains(&ptr)
        || (0x4000_0000..0x4018_0000).contains(&ptr);
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

static WDEV_RING: Ring = Ring::new();
static PPHDR_RING: Ring = Ring::new();
static PPRX_RING: Ring = Ring::new();
static PPRX_PROTO_RING: Ring = Ring::new();
static PPRX_FRAG_RING: Ring = Ring::new();
static PPDEQ_RING: Ring = Ring::new();
static STA_INPUT_RING: Ring = Ring::new();

unsafe extern "C" {
    fn __real_wdevProcessRxSucDataAll(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_ppProcessRxPktHdr(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_ppRxPkt(a2: usize, a3: usize, a4: usize, a5: usize, a6: usize, a7: usize) -> usize;
    fn __real_ppRxProtoProc(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_ppRxFragmentProc(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_ppDequeueRxq_Locked(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
    fn __real_sta_input(
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> usize;
}

fn snapshot_call(
    real: unsafe extern "C" fn(usize, usize, usize, usize, usize, usize) -> usize,
    ring: &Ring,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    let pre_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (pre_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    let ret = unsafe { real(a2, a3, a4, a5, a6, a7) };
    let arg3_words = capture_words(a5);
    let ret_words = capture_words(ret);
    let post_mac_isr = esp_wifi::diagnostic_wifi_mac_isr_count() as u32;
    let (post_rx_sta, _) = esp_wifi::diagnostic_wifi_rx_cb_counts();
    ring.push(Snapshot {
        args: [a2 as u32, a3 as u32, a4 as u32, a5 as u32],
        ret: ret as u32,
        arg3_words,
        ret_words,
        pre_mac_isr,
        post_mac_isr,
        pre_rx_sta: pre_rx_sta as u32,
        post_rx_sta: post_rx_sta as u32,
    });
    ret
}

pub(crate) fn reset_rx_dispatch_wrap_diag() {
    WDEV_RING.reset();
    PPHDR_RING.reset();
    PPRX_RING.reset();
    PPRX_PROTO_RING.reset();
    PPRX_FRAG_RING.reset();
    PPDEQ_RING.reset();
    STA_INPUT_RING.reset();
}

pub(crate) fn print_rx_dispatch_wrap_diag(label: &str, phase: &str) {
    WDEV_RING.print(label, phase, "wdev_rx_wrap_diag");
    PPHDR_RING.print(label, phase, "pphdr_wrap_diag");
    PPRX_RING.print(label, phase, "pprx_wrap_diag");
    PPRX_PROTO_RING.print(label, phase, "pprx_proto_wrap_diag");
    PPRX_FRAG_RING.print(label, phase, "pprx_frag_wrap_diag");
    PPDEQ_RING.print(label, phase, "ppdeq_wrap_diag");
    STA_INPUT_RING.print(label, phase, "sta_input_wrap_diag");
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_wdevProcessRxSucDataAll(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    snapshot_call(
        __real_wdevProcessRxSucDataAll,
        &WDEV_RING,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ppProcessRxPktHdr(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    snapshot_call(__real_ppProcessRxPktHdr, &PPHDR_RING, a2, a3, a4, a5, a6, a7)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ppRxPkt(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    snapshot_call(__real_ppRxPkt, &PPRX_RING, a2, a3, a4, a5, a6, a7)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ppRxProtoProc(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    snapshot_call(
        __real_ppRxProtoProc,
        &PPRX_PROTO_RING,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ppRxFragmentProc(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    snapshot_call(
        __real_ppRxFragmentProc,
        &PPRX_FRAG_RING,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_ppDequeueRxq_Locked(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    snapshot_call(
        __real_ppDequeueRxq_Locked,
        &PPDEQ_RING,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __wrap_sta_input(
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> usize {
    snapshot_call(__real_sta_input, &STA_INPUT_RING, a2, a3, a4, a5, a6, a7)
}
