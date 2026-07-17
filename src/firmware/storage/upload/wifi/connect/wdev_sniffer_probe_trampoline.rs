use core::sync::atomic::{AtomicUsize, Ordering};

use esp_println::println;

const RING_LEN: usize = 8;

#[derive(Copy, Clone)]
struct SnifferEntry {
    a10: u32,
    a11: u32,
    meta48: u8,
    meta52: u32,
}

impl SnifferEntry {
    const ZERO: Self = Self {
        a10: 0,
        a11: 0,
        meta48: 0,
        meta52: 0,
    };
}

struct Ring {
    next: AtomicUsize,
    entries: [SnifferEntry; RING_LEN],
}

impl Ring {
    const fn new() -> Self {
        Self {
            next: AtomicUsize::new(0),
            entries: [SnifferEntry::ZERO; RING_LEN],
        }
    }

    fn reset(&self) {
        self.next.store(0, Ordering::Relaxed);
        unsafe {
            core::ptr::write(
                self.entries.as_ptr() as *mut [SnifferEntry; RING_LEN],
                [SnifferEntry::ZERO; RING_LEN],
            );
        }
    }

    fn push(&self, entry: SnifferEntry) {
        let idx = self.next.fetch_add(1, Ordering::Relaxed);
        if idx < RING_LEN {
            unsafe {
                core::ptr::write(self.entries.as_ptr().add(idx) as *mut SnifferEntry, entry);
            }
        }
    }

    fn log(&self, stage: &str) {
        let count = self.next.load(Ordering::Relaxed).min(RING_LEN);
        println!(
            "upload_http: boot_scan_only_diag wdev_sniffer_probe after={} count={}",
            stage, count
        );
        for idx in 0..count {
            let entry = unsafe { core::ptr::read(self.entries.as_ptr().add(idx)) };
            println!(
                "upload_http: boot_scan_only_diag wdev_sniffer_probe_entry after={} idx={} a10=0x{:08x} a11=0x{:08x} meta48=0x{:02x} meta52=0x{:08x}",
                stage,
                idx,
                entry.a10,
                entry.a11,
                entry.meta48,
                entry.meta52
            );
        }
    }
}

static SNIFFER_RING: Ring = Ring::new();

unsafe extern "C" {
    #[link_name = "wDev_SnifferRxData"]
    fn wdev_sniffer_rxdata_real(a10: usize, a11: usize) -> usize;
}

#[no_mangle]
pub unsafe extern "C" fn wdev_sniffer_probe_trampoline(a10: usize, a11: usize) -> usize {
    let mut meta48 = 0u8;
    let mut meta52 = 0u32;
    if a10 != 0 {
        meta48 = core::ptr::read_unaligned((a10 as *const u8).add(48));
        meta52 = core::ptr::read_unaligned((a10 as *const u32).add(13));
    }
    SNIFFER_RING.push(SnifferEntry {
        a10: a10 as u32,
        a11: a11 as u32,
        meta48,
        meta52,
    });
    unsafe { wdev_sniffer_rxdata_real(a10, a11) }
}

// Keep the trampoline symbol from dead stripping; it is referenced via a binary patch.
#[used]
#[no_mangle]
static WDEV_SNIFFER_PROBE_TRAMPOLINE_KEEP: unsafe extern "C" fn(usize, usize) -> usize =
    wdev_sniffer_probe_trampoline;

pub(super) fn reset_wdev_sniffer_probe_trampoline() {
    keep_wdev_sniffer_probe_trampoline();
    SNIFFER_RING.reset();
}

pub(super) fn log_wdev_sniffer_probe_trampoline(stage: &str) {
    SNIFFER_RING.log(stage);
}

#[inline(never)]
fn keep_wdev_sniffer_probe_trampoline() {
    let func = wdev_sniffer_probe_trampoline as usize;
    unsafe {
        core::ptr::read_volatile(&func);
    }
}
